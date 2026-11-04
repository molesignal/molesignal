// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 拿转换结果和对方自己排出来的值班表比。文件里带了对方的渲染结果时，预演就能当场发现转换和来源对不上。
//!
//! 只比现在往后的时段：过去的班不会再有人被通知。临时替班不导入，它占的时段不比。

use chrono::{Duration, Offset, TimeZone};
use chrono_tz::Tz;

use super::Draft;
use crate::{
    app::alerting::oncall_transfer::Finding,
    domain::alerting::schedule::Schedule,
    shared::{ids::Id, time::TimestampMicros},
};

/// 最多列出几段对不上的。
const MAX_MISMATCHES: usize = 3;
/// 没有值班人时的写法。
const NOBODY: &str = "–";

/// 对方排出来的一段班；`owner` 是邮箱，空着是没人值班。
pub(super) struct RenderedShift {
    pub start: i64,
    pub end: i64,
    pub owner: Option<String>,
}

/// 对方排出来的值班表。
pub(super) struct Rendered {
    pub from: i64,
    pub to: i64,
    pub shifts: Vec<RenderedShift>,
    /// 不比的时段：临时替班，以及 Opsgenie 的转发。
    pub excluded: Vec<(i64, i64)>,
}

/// 比完把结果记到排班上：对不上的挡住写入，其余的说明比过什么、没比什么。
pub(super) fn check(draft: &mut Draft<'_>, tz: Tz, rendered: Option<Rendered>, now: i64) {
    let Some(rendered) = rendered else {
        draft.note(Finding::new("rendered_missing"));
        return;
    };

    let from = rendered.from.max(now);

    if from >= rendered.to {
        draft.note(Finding::new("rendered_stale"));
        return;
    }

    let mismatches = mismatches(draft, tz, &rendered, from);

    if mismatches.is_empty() {
        draft.note(
            Finding::new("rendered_matched")
                .with("from", super::local_text(from, tz))
                .with("to", super::local_text(rendered.to, tz)),
        );

        if changes_clock(tz, now, now + YEAR) && !changes_clock(tz, from, rendered.to) {
            draft.note(Finding::new("clock_change_not_compared").with("timezone", tz.name()));
        }

        return;
    }

    for mismatch in mismatches.iter().take(MAX_MISMATCHES) {
        draft.block(
            Finding::new("rendered_mismatch")
                .with("at", super::local_text(mismatch.start, tz))
                .with("until", super::local_text(mismatch.end, tz))
                .with("expected", mismatch.expected.as_deref().unwrap_or(NOBODY))
                .with("actual", mismatch.actual.as_deref().unwrap_or(NOBODY)),
        );
    }

    if mismatches.len() > MAX_MISMATCHES {
        draft.block(
            Finding::new("rendered_mismatch_more")
                .with("count", (mismatches.len() - MAX_MISMATCHES).to_string()),
        );
    }
}

const YEAR: i64 = 366 * 86_400_000_000;

/// `[from, to)` 里这个时区拨过钟。
fn changes_clock(tz: Tz, from: i64, to: i64) -> bool {
    let offset = |at: i64| {
        TimestampMicros(at)
            .to_datetime()
            .with_timezone(&tz)
            .offset()
            .fix()
    };
    let start = offset(from);

    (0..)
        .map(|week| from + week * 7 * 86_400_000_000)
        .take_while(|at| *at < to)
        .chain([to])
        .any(|at| offset(at) != start)
}

/// 响应里的时间带着渲染它用的偏移。这个时区一年里都不用这个偏移，说明响应是按别的时区渲染的。
pub(super) fn offset_is_foreign(text: &str, tz: Tz) -> bool {
    let Ok(at) = chrono::DateTime::parse_from_rfc3339(text) else {
        return false;
    };
    let offset = at.offset().local_minus_utc();
    let instant = at.naive_utc();

    (0..=12).all(|month| {
        tz.offset_from_utc_datetime(&(instant + Duration::days(30 * month)))
            .fix()
            .local_minus_utc()
            != offset
    })
}

struct Mismatch {
    start: i64,
    end: i64,
    expected: Option<String>,
    actual: Option<String>,
}

/// 转换结果和对方的渲染结果在 `[from, rendered.to)` 里哪几段不一样。这时成员还是邮箱。
fn mismatches(draft: &Draft<'_>, tz: Tz, rendered: &Rendered, from: i64) -> Vec<Mismatch> {
    let schedule = Schedule {
        id: Id::from_string("foreign"),
        org_id: Id::from_string("foreign"),
        name: draft.name.clone(),
        description: String::new(),
        team_id: None,
        timezone: tz.name().to_string(),
        enabled: true,
        rotations: draft.rotations.clone(),
        overrides: Vec::new(),
        swap_needs_approval: false,
        created_by: None,
        updated_by: None,
        created_at: TimestampMicros(0),
        updated_at: TimestampMicros(0),
    };
    let segments = schedule.segments(TimestampMicros(from), TimestampMicros(rendered.to));

    let mut shifts: Vec<&RenderedShift> = rendered.shifts.iter().collect();

    shifts.sort_by_key(|shift| shift.start);

    let mut points: Vec<i64> = segments.iter().map(|segment| segment.start_at.0).collect();

    points.extend(shifts.iter().flat_map(|shift| [shift.start, shift.end]));
    points.extend(
        rendered
            .excluded
            .iter()
            .flat_map(|(start, end)| [*start, *end]),
    );
    points.push(from);
    points.retain(|at| *at >= from && *at < rendered.to);
    points.sort_unstable();
    points.dedup();

    let excluded = merged(&rendered.excluded);
    let mut found: Vec<Mismatch> = Vec::new();

    for (index, &start) in points.iter().enumerate() {
        if covers(&excluded, start) {
            continue;
        }

        let end = points.get(index + 1).copied().unwrap_or(rendered.to);
        let expected = rendered_owner(&shifts, start);
        let held = segments.partition_point(|segment| segment.start_at.0 <= start);
        let actual = segments[..held]
            .last()
            .filter(|segment| start < segment.end_at.0)
            .and_then(|segment| segment.user_id.as_ref())
            .map(|id| id.0.to_lowercase());

        if expected == actual {
            continue;
        }

        match found.last_mut() {
            Some(last)
                if last.end == start && last.expected == expected && last.actual == actual =>
            {
                last.end = end;
            }
            _ => found.push(Mismatch {
                start,
                end,
                expected,
                actual,
            }),
        }
    }

    found
}

/// 按开始排好序，相连的并成一段，好用二分找。
fn merged(spans: &[(i64, i64)]) -> Vec<(i64, i64)> {
    let mut sorted = spans.to_vec();

    sorted.sort_unstable();

    let mut joined: Vec<(i64, i64)> = Vec::with_capacity(sorted.len());

    for (start, end) in sorted {
        match joined.last_mut() {
            Some(last) if start <= last.1 => last.1 = last.1.max(end),
            _ => joined.push((start, end)),
        }
    }

    joined
}

fn covers(spans: &[(i64, i64)], at: i64) -> bool {
    let started = spans.partition_point(|(start, _)| *start <= at);

    spans[..started].last().is_some_and(|(_, end)| at < *end)
}

fn rendered_owner(shifts: &[&RenderedShift], at: i64) -> Option<String> {
    let index = shifts.partition_point(|shift| shift.start <= at);

    shifts[..index]
        .last()
        .filter(|shift| at < shift.end)
        .and_then(|shift| shift.owner.as_ref())
        .map(|owner| owner.to_lowercase())
}
