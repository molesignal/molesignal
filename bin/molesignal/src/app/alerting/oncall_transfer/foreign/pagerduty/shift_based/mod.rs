// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! PagerDuty 的 shift-based 排班：`GET /v3/schedules/{id}` 响应里的事件（`rotations[].events`）换成轮值。
//!
//! - 一个事件换成一条轮值。事件之间没有优先级，时间上重叠就是几个人同时值班；一份排班同一时刻只有一个值班人，所以
//!   重叠的挡住写入，时段互不重叠的（工作时间一个、下班时间一个）顺序无所谓。
//! - `start_time`、`end_time` 是第一班的起止，`recurrence` 说它怎么重复，见 [`recurrence`]。轮到谁从 `start_time`
//!   数起；`effective_since`、`effective_until` 只决定事件什么时候产生班次，过去的 `effective_since` 还会被
//!   PagerDuty 改成当时，不能用来算轮到谁。
//! - 时区写在事件的 `start_time` 上，可以和排班的不一样。交接跟着事件的时区走，所以转换结果用它。
//! - `every_member_assignment_strategy` 是所有成员同时值班，`empty_member` 是故意空着的一班，轮值都表达不了。
//! - 响应里的成员只有 `user_id`，邮箱要从 `GET /users` 取，放在文件顶层的 `users` 里：`include[]=users` 给的也只是
//!   引用，没有邮箱。

use chrono_tz::Tz;
use serde::Deserialize;
use serde_json::Value;

use self::{
    final_schedule::FinalSchedule,
    recurrence::{FirstShift, cadence_of},
};
use super::{
    super::{
        Draft, ForeignSchedule, kind_of_period, local_text, overlap::check_overlaps, parse_time,
        weekly::WeekMinutes,
    },
    Emails, Reference, starting_later, unreadable,
};
use crate::{
    app::alerting::oncall_transfer::{Directory, Finding},
    domain::alerting::{schedule::Rotation, time_window::parse_timezone},
    shared::{ids::Id, time::TimestampMicros},
};

mod final_schedule;
mod recurrence;

#[derive(Deserialize)]
struct Schedule {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    time_zone: Option<String>,
    #[serde(default)]
    teams: Vec<Reference>,
    #[serde(default)]
    rotations: Option<Vec<SourceRotation>>,
    #[serde(default)]
    final_schedule: Option<FinalSchedule>,
}

#[derive(Deserialize)]
struct SourceRotation {
    #[serde(default)]
    events: Option<Vec<Event>>,
}

#[derive(Deserialize)]
struct Event {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    start_time: ZonedTime,
    end_time: ZonedTime,
    #[serde(default)]
    effective_since: Option<String>,
    #[serde(default)]
    effective_until: Option<String>,
    #[serde(default)]
    recurrence: Vec<String>,
    assignment_strategy: AssignmentStrategy,
}

#[derive(Deserialize)]
struct ZonedTime {
    date_time: String,
    #[serde(default)]
    time_zone: Option<String>,
}

#[derive(Deserialize)]
struct AssignmentStrategy {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    shifts_per_member: Option<i64>,
    #[serde(default)]
    members: Vec<Member>,
}

#[derive(Deserialize)]
struct Member {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    user_id: Option<String>,
}

/// 还在生效的事件和读出来的时间。
struct Running<'a> {
    source: &'a Event,
    name: String,
    start: i64,
    end: i64,
    since: Option<i64>,
    until: Option<i64>,
    zone: Tz,
}

pub(super) fn read(
    raw: &Value,
    emails: &Emails,
    window: Option<(i64, i64)>,
    directory: &Directory,
    now: i64,
) -> ForeignSchedule {
    match serde_json::from_value::<Schedule>(raw.clone()) {
        Ok(schedule) => convert(&schedule, emails, window, directory, now),
        Err(error) => unreadable(raw, &error.to_string(), directory),
    }
}

fn convert(
    schedule: &Schedule,
    emails: &Emails,
    window: Option<(i64, i64)>,
    directory: &Directory,
    now: i64,
) -> ForeignSchedule {
    let mut draft = Draft::new(
        directory,
        schedule.name.as_deref().or(schedule.id.as_deref()),
        schedule.time_zone.as_deref(),
    );

    draft.description = schedule.description.clone().filter(|text| !text.is_empty());
    draft.adopt_team(
        schedule
            .teams
            .iter()
            .filter_map(|team| team.summary.as_deref()),
    );

    let Some(schedule_zone) = draft.zone() else {
        return draft.finish();
    };

    let Some(events) = events_of(schedule) else {
        draft.block(Finding::new("events_missing"));
        return draft.finish();
    };

    let running: Vec<Running<'_>> = events
        .into_iter()
        .enumerate()
        .filter_map(|(index, event)| running_of(&mut draft, event, index, schedule_zone, now))
        .collect();

    let Some(tz) = shared_zone(&mut draft, &running, schedule_zone) else {
        return draft.finish();
    };

    let mut weeks = Vec::new();

    for event in &running {
        if draft.rotations_full() {
            break;
        }

        add_event(&mut draft, event, &mut weeks, emails, tz, now);
    }

    check_overlaps(&mut draft, &weeks, tz, now);

    final_schedule::compare(
        &mut draft,
        schedule.final_schedule.as_ref(),
        emails,
        window,
        tz,
        now,
    );

    draft.finish()
}

/// 所有轮换的事件。文件里缺轮换或事件时为空：不是没人值班，而是说不清谁值班。
fn events_of(schedule: &Schedule) -> Option<Vec<&Event>> {
    let mut events = Vec::new();

    for rotation in schedule.rotations.as_ref()? {
        events.extend(rotation.events.as_ref()?);
    }

    Some(events)
}

/// 读一个时刻；不是时间的挡住写入。
fn time_of(draft: &mut Draft<'_>, text: &str, name: &str) -> Option<i64> {
    let at = parse_time(text);

    if at.is_none() {
        draft.block(
            Finding::new("time_invalid")
                .with("rotation", name)
                .with("value", text),
        );
    }

    at
}

/// 读事件的时间。已经结束的事件不影响现在往后谁值班，它的内容也就不用查了。
fn running_of<'a>(
    draft: &mut Draft<'_>,
    source: &'a Event,
    index: usize,
    schedule_zone: Tz,
    now: i64,
) -> Option<Running<'a>> {
    let name = source
        .name
        .clone()
        .unwrap_or_else(|| format!("Event {}", index + 1));

    let start = time_of(draft, &source.start_time.date_time, &name)?;
    let end = time_of(draft, &source.end_time.date_time, &name)?;

    let since = match source.effective_since.as_deref() {
        Some(text) => Some(time_of(draft, text, &name)?),
        None => None,
    };

    let until = match source.effective_until.as_deref() {
        Some(text) => Some(time_of(draft, text, &name)?),
        None => None,
    };

    let first_shift = since.map_or(start, |since| since.max(start));

    if let Some(until) = until.filter(|until| *until <= now || *until <= first_shift) {
        draft.note(
            Finding::new("rotation_ended")
                .with("rotation", name)
                .with("at", local_text(until, schedule_zone)),
        );

        return None;
    }

    let zone = match source.start_time.time_zone.as_deref() {
        None => schedule_zone,
        Some(text) => match parse_timezone(text) {
            Ok(zone) => zone,
            Err(_) => {
                draft.block(Finding::new("timezone_unknown").with("timezone", text));
                return None;
            }
        },
    };

    Some(Running {
        source,
        name,
        start,
        end,
        since,
        until,
        zone,
    })
}

/// 事件都用一个时区时转换结果就用它，交接时间才不变。几个时区挡住写入：一份排班只有一个时区。
fn shared_zone(draft: &mut Draft<'_>, running: &[Running<'_>], schedule_zone: Tz) -> Option<Tz> {
    let Some(first) = running.first() else {
        return Some(schedule_zone);
    };

    if let Some(other) = running.iter().find(|event| event.zone != first.zone) {
        draft.block(
            Finding::new("event_timezones_differ")
                .with("rotation", first.name.clone())
                .with("timezone", first.zone.name())
                .with("other", other.name.clone())
                .with("other_timezone", other.zone.name()),
        );

        return None;
    }

    if first.zone != schedule_zone {
        draft.note(
            Finding::new("timezone_from_events")
                .with("schedule_timezone", schedule_zone.name())
                .with("timezone", first.zone.name()),
        );
        draft.timezone = first.zone.name().to_string();
    }

    Some(first.zone)
}

fn add_event(
    draft: &mut Draft<'_>,
    event: &Running<'_>,
    weeks: &mut Vec<WeekMinutes>,
    emails: &Emails,
    tz: Tz,
    now: i64,
) {
    let name = event.name.as_str();
    let strategy = &event.source.assignment_strategy;
    let before = draft.blockers.len();
    let members = members_of(draft, strategy, name, emails);
    let several = strategy.members.len() > 1;

    let Some(shifts) = shifts_of(draft, strategy, name, several) else {
        return;
    };

    let first = FirstShift {
        start: event.start,
        end: event.end,
        tz,
    };

    let cadence = match cadence_of(&event.source.recurrence, &first, shifts, several) {
        Ok(cadence) => cadence,
        Err(finding) => {
            draft.block(finding.with("rotation", name));
            return;
        }
    };

    if draft.blockers.len() != before {
        return;
    }

    // 没人的事件不会有人值班。
    if members.is_empty() {
        draft.note(Finding::new("rotation_empty").with("rotation", name));
        return;
    }

    let rotation = Rotation {
        id: Id::from_string(event.source.id.clone().unwrap_or_else(|| name.to_string())),
        name: name.to_string(),
        members,
        kind: kind_of_period(cadence.turn_secs),
        active_windows: cadence.minutes.ranges(),
        calendar: None,
        start_at: TimestampMicros(event.start),
        end_at: event.until.map(TimestampMicros),
    };

    let later = event
        .since
        .filter(|since| *since > event.start && *since > now);

    let added = match later {
        Some(since) => starting_later(rotation, since, tz),
        None => vec![rotation],
    };

    weeks.extend(added.iter().map(|_| cadence.minutes.clone()));
    draft.rotations.extend(added);
}

fn members_of(
    draft: &mut Draft<'_>,
    strategy: &AssignmentStrategy,
    name: &str,
    emails: &Emails,
) -> Vec<Id> {
    let is_empty = |member: &Member| member.kind == "empty_member";

    // 全是空位的事件不会有人值班，和没有这个事件一样。
    if strategy.members.iter().any(is_empty) && !strategy.members.iter().all(is_empty) {
        draft.block(Finding::new("participant_none").with("rotation", name));
    }

    let mut members = Vec::new();

    for member in strategy.members.iter().filter(|member| !is_empty(member)) {
        match (member.kind.as_str(), member.user_id.as_deref()) {
            ("user_member", Some(id)) => match emails.get(id) {
                Some(email) => members.push(draft.member(email)),
                None => draft.block(
                    Finding::new("user_without_email")
                        .with("rotation", name)
                        .with("user", id),
                ),
            },
            (other, _) => draft.block(
                Finding::new("participant_unknown")
                    .with("rotation", name)
                    .with("value", other),
            ),
        }
    }

    members
}

/// 每人连值几班。只有一个人时交接没有意义，算一班。写不成轮值的策略挡住写入。
fn shifts_of(
    draft: &mut Draft<'_>,
    strategy: &AssignmentStrategy,
    name: &str,
    several: bool,
) -> Option<u64> {
    match strategy.kind.as_str() {
        "rotating_member_assignment_strategy" if several => {
            let shifts = strategy
                .shifts_per_member
                .and_then(|shifts| u64::try_from(shifts).ok())
                .filter(|shifts| *shifts > 0);

            if shifts.is_none() {
                let value = strategy
                    .shifts_per_member
                    .map_or_else(|| "null".to_string(), |shifts| shifts.to_string());

                draft.block(
                    Finding::new("rotation_length_invalid")
                        .with("rotation", name)
                        .with("value", value),
                );
            }

            shifts
        }
        "every_member_assignment_strategy" if several => {
            draft.block(Finding::new("every_member_on_call").with("rotation", name));
            None
        }
        "rotating_member_assignment_strategy" | "every_member_assignment_strategy" => Some(1),
        other => {
            draft.block(
                Finding::new("rotation_type_unsupported")
                    .with("rotation", name)
                    .with("value", other),
            );

            None
        }
    }
}
