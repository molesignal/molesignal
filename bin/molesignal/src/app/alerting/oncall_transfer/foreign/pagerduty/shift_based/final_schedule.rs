// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! PagerDuty 自己排出来的值班表：`final_schedule.computed_shift_assignments`。几条轮换、自定义班次和替班都合在这一个
//! 列表里，每条的 `source` 说它的来历。
//!
//! - 同一时刻可以有几条：几个人同时值班。这里写不出来，还没过去的挡住写入。
//! - 没人值班的时段也列出来（`empty_member`），和没有这一段是一个意思。
//! - 替班（`schedule_rotation_override`）不导入，它占的时段不比，和 V2 一样。
//! - 自定义班次（`custom_shift`）是一次性加的人，也可能是一次性的替班，都不在事件里，只有这里看得到。轮值表达不了，
//!   还没过去的挡住写入。

use chrono_tz::Tz;
use serde::Deserialize;

use super::{
    super::{
        super::{
            Draft, local_text, parse_time,
            rendered::{self, Rendered, RenderedShift},
        },
        Emails,
    },
    Member,
};
use crate::app::alerting::oncall_transfer::Finding;

#[derive(Deserialize)]
pub(super) struct FinalSchedule {
    #[serde(default)]
    computed_shift_assignments: Vec<Assignment>,
}

#[derive(Deserialize)]
struct Assignment {
    start_time: String,
    end_time: String,
    member: Member,
    #[serde(default)]
    source: Option<Source>,
}

#[derive(Deserialize)]
struct Source {
    #[serde(rename = "type", default)]
    kind: Option<String>,
}

/// 按来历分开的值班表。
#[derive(Default)]
struct Sources {
    rotations: Vec<RenderedShift>,
    custom: Vec<RenderedShift>,
    overrides: Vec<(i64, i64)>,
}

/// 把对方排出来的值班表和转换结果比，结果记到排班上。已经挡住写入的排班不用再比。
pub(super) fn compare(
    draft: &mut Draft<'_>,
    final_schedule: Option<&FinalSchedule>,
    emails: &Emails,
    window: Option<(i64, i64)>,
    tz: Tz,
    now: i64,
) {
    let Some(final_schedule) = final_schedule else {
        if draft.blockers.is_empty() {
            rendered::check(draft, tz, None, now);
        }

        return;
    };

    let Sources {
        rotations,
        custom,
        overrides,
    } = sources_of(final_schedule, emails);

    block_custom_shifts(draft, &custom, tz, now);

    // 没有脚本给的范围时，只比最早一段到最晚一段之间。
    let window = window.or_else(|| {
        Some((
            rotations.iter().map(|shift| shift.start).min()?,
            rotations.iter().map(|shift| shift.end).max()?,
        ))
    });

    let overrides_ahead = overrides.iter().filter(|(_, end)| *end > now).count();

    if draft.blockers.is_empty() {
        match one_at_a_time(rotations, tz, now) {
            Ok(shifts) => {
                let rendered = window.map(|(from, to)| Rendered {
                    from,
                    to,
                    shifts,
                    excluded: overrides,
                });

                rendered::check(draft, tz, rendered, now);
            }
            Err(finding) => draft.block(finding),
        }
    }

    if overrides_ahead > 0 {
        draft.note(Finding::new("overrides_left_out").with("count", overrides_ahead.to_string()));
    }
}

/// 认不出的来历也当作轮换的班次：拿去比，对不上就会挡住，不会悄悄放过。
fn sources_of(final_schedule: &FinalSchedule, emails: &Emails) -> Sources {
    let mut sources = Sources::default();

    for assignment in &final_schedule.computed_shift_assignments {
        let (Some(start), Some(end)) = (
            parse_time(&assignment.start_time),
            parse_time(&assignment.end_time),
        ) else {
            continue;
        };

        let kind = assignment
            .source
            .as_ref()
            .and_then(|source| source.kind.as_deref());

        let shift = owner_of(&assignment.member, emails).map(|owner| RenderedShift {
            start,
            end,
            owner: Some(owner),
        });

        match kind {
            Some("schedule_rotation_override") => sources.overrides.push((start, end)),
            Some("custom_shift" | "custom_shift_override") => sources.custom.extend(shift),
            _ => sources.rotations.extend(shift),
        }
    }

    sources
}

/// 值班人的邮箱；文件里没有邮箱时用 id。没人值班时为空。
fn owner_of(member: &Member, emails: &Emails) -> Option<String> {
    let id = member
        .user_id
        .as_ref()
        .filter(|_| member.kind == "user_member")?;

    Some(emails.get(id).unwrap_or(id).clone())
}

fn block_custom_shifts(draft: &mut Draft<'_>, custom: &[RenderedShift], tz: Tz, now: i64) {
    let ahead: Vec<&RenderedShift> = custom.iter().filter(|shift| shift.end > now).collect();

    if let Some(first) = ahead.iter().min_by_key(|shift| shift.start) {
        draft.block(
            Finding::new("custom_shifts")
                .with("count", ahead.len().to_string())
                .with("at", local_text(first.start.max(now), tz))
                .with("until", local_text(first.end, tz))
                .with("user", first.owner.clone().unwrap_or_default()),
        );
    }
}

/// 现在往后的班次按开始排好，同一个人相连或重叠的并成一段，好给 [`rendered::check`] 比。不同的人重叠就是同时值班，
/// 这里写不出来。
fn one_at_a_time(
    shifts: Vec<RenderedShift>,
    tz: Tz,
    now: i64,
) -> Result<Vec<RenderedShift>, Finding> {
    let mut ahead: Vec<RenderedShift> = shifts
        .into_iter()
        .filter(|shift| shift.start < shift.end && now < shift.end)
        .collect();

    ahead.sort_by_key(|shift| shift.start);

    let mut joined: Vec<RenderedShift> = Vec::with_capacity(ahead.len());

    for shift in ahead {
        match joined.last_mut() {
            Some(last) if shift.start < last.end && shift.owner != last.owner => {
                return Err(Finding::new("rendered_several_on_call")
                    .with("at", local_text(shift.start.max(now), tz))
                    .with("until", local_text(shift.end.min(last.end), tz))
                    .with("first", last.owner.clone().unwrap_or_default())
                    .with("second", shift.owner.unwrap_or_default()));
            }
            Some(last) if shift.start <= last.end && shift.owner == last.owner => {
                last.end = last.end.max(shift.end);
            }
            _ => joined.push(shift),
        }
    }

    Ok(joined)
}
