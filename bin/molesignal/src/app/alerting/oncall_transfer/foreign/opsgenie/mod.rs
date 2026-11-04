// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Opsgenie 的排班：`GET /v2/schedules/{id}` 响应里的轮换（`rotations`）换成轮值。
//!
//! - 轮换的类型（`hourly`、`daily`、`weekly`）乘 `length` 是周期，`startDate` 是第一次交接，都按排班时区的
//!   本地时间。参与者的 `username` 就是邮箱。
//! - 轮换有重叠时 Opsgenie 让几个人同时值班。一份排班同一时刻只有一个值班人，所以重叠的轮换挡住写入；时段互不
//!   重叠的轮换（工作时间一条、下班时间一条）顺序无所谓。
//! - 团队、升级策略和「没有人」作参与者，轮值都表达不了。
//! - `GET /v2/schedules/{id}/timeline` 的结果放在文件顶层的 `timelines` 里，预演拿它对照。

use chrono_tz::Tz;
use serde::Deserialize;
use serde_json::Value;

use self::{
    restrictions::{TimeRestriction, restricted_minutes},
    timeline::{Timeline, belongs_to, rendered_of},
};
use super::{
    Draft, ForeignImport, ForeignSchedule, OPSGENIE_FORMAT, kind_of_period, local_text,
    overlap::check_overlaps, parse_time, rendered, weekly::WeekMinutes,
};
use crate::{
    app::alerting::oncall_transfer::{Directory, Finding},
    domain::alerting::schedule::Rotation,
    shared::{ids::Id, time::TimestampMicros},
};

mod restrictions;
mod timeline;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Schedule {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    timezone: Option<String>,
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    owner_team: Option<Team>,
    #[serde(default)]
    rotations: Option<Vec<SourceRotation>>,
}

#[derive(Deserialize)]
struct Team {
    #[serde(default)]
    name: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SourceRotation {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    start_date: String,
    #[serde(default)]
    end_date: Option<String>,
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    length: Option<i64>,
    #[serde(default)]
    participants: Vec<Participant>,
    #[serde(default)]
    time_restriction: Option<TimeRestriction>,
}

#[derive(Deserialize)]
struct Participant {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    username: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

pub(super) fn recognizes(bundle: &Value) -> bool {
    match bundle.get("data") {
        Some(Value::Array(_)) => true,
        Some(Value::Object(schedule)) => {
            schedule.contains_key("rotations") || schedule.contains_key("timezone")
        }
        _ => false,
    }
}

pub(super) fn read(bundle: &Value, directory: &Directory, now: i64) -> ForeignImport {
    let timelines: Vec<Timeline> = bundle
        .get("timelines")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|raw| serde_json::from_value(raw.clone()).ok())
        .collect();

    let raws: Vec<&Value> = match bundle.get("data") {
        Some(Value::Array(items)) => items.iter().collect(),
        Some(single) => vec![single],
        None => Vec::new(),
    };

    let schedules = raws
        .into_iter()
        .map(
            |raw| match serde_json::from_value::<Schedule>(raw.clone()) {
                Ok(schedule) => convert(&schedule, &timelines, directory, now),
                Err(error) => unreadable(raw, &error.to_string(), directory),
            },
        )
        .collect();

    ForeignImport {
        format: OPSGENIE_FORMAT,
        schedules,
        left_out: Vec::new(),
        findings: vec![Finding::new("schedules_only")],
    }
}

fn unreadable(raw: &Value, error: &str, directory: &Directory) -> ForeignSchedule {
    let mut draft = Draft::new(directory, raw.get("name").and_then(Value::as_str), None);

    draft.block(Finding::new("schedule_unreadable").with("detail", error));
    draft.finish()
}

fn convert(
    schedule: &Schedule,
    timelines: &[Timeline],
    directory: &Directory,
    now: i64,
) -> ForeignSchedule {
    let mut draft = Draft::new(
        directory,
        schedule.name.as_deref(),
        schedule.timezone.as_deref(),
    );

    draft.description = schedule.description.clone().filter(|text| !text.is_empty());
    draft.enabled = schedule.enabled;
    draft.adopt_team(
        schedule
            .owner_team
            .iter()
            .filter_map(|team| team.name.as_deref()),
    );

    let Some(tz) = draft.zone() else {
        return draft.finish();
    };

    let Some(rotations) = &schedule.rotations else {
        draft.block(Finding::new("rotations_missing"));
        return draft.finish();
    };

    let mut weeks = Vec::new();

    for (index, source) in rotations.iter().enumerate() {
        if draft.rotations_full() {
            break;
        }

        let name = source
            .name
            .clone()
            .unwrap_or_else(|| format!("Rotation {}", index + 1));

        if let Some(week) = add_rotation(&mut draft, source, &name, tz, now) {
            weeks.push(week);
        }
    }

    check_overlaps(&mut draft, &weeks, tz, now);

    // 停用的排班两边都没人值班，比了也说明不了什么。
    if schedule.enabled == Some(false) {
        draft.note(Finding::new("schedule_paused"));
    } else if draft.blockers.is_empty() {
        let timeline = timelines
            .iter()
            .find(|timeline| belongs_to(timeline, schedule));

        rendered::check(&mut draft, tz, timeline.and_then(rendered_of), now);
    }

    draft.finish()
}

/// 换一条轮换，得到它一周里生效的分钟。结束了的、表达不了的不算。
fn add_rotation(
    draft: &mut Draft<'_>,
    source: &SourceRotation,
    name: &str,
    tz: Tz,
    now: i64,
) -> Option<WeekMinutes> {
    let invalid = |value: &str| {
        Finding::new("time_invalid")
            .with("rotation", name)
            .with("value", value)
    };

    let Some(start) = parse_time(&source.start_date) else {
        draft.block(invalid(&source.start_date));
        return None;
    };

    let end = match source.end_date.as_deref() {
        None => None,
        Some(text) => match parse_time(text) {
            Some(end) => Some(end),
            None => {
                draft.block(invalid(text));
                return None;
            }
        },
    };

    // 已经结束的轮换不影响现在往后谁值班，它的内容也就不用查了。
    if let Some(end) = end.filter(|end| *end <= now || *end <= start) {
        draft.note(
            Finding::new("rotation_ended")
                .with("rotation", name)
                .with("at", local_text(end, tz)),
        );
        return None;
    }

    let before = draft.blockers.len();
    let members = members_of(draft, source, name);
    let period = period_of(draft, source, name);
    let minutes = match &source.time_restriction {
        Some(restriction) => match restricted_minutes(restriction) {
            Ok(minutes) => minutes,
            Err(finding) => {
                draft.block(finding.with("rotation", name));
                WeekMinutes::empty()
            }
        },
        None => WeekMinutes::full(),
    };

    let (Some(period), true) = (period, draft.blockers.len() == before) else {
        return None;
    };

    if members.is_empty() || minutes.is_empty() {
        draft.note(Finding::new("rotation_empty").with("rotation", name));
        return None;
    }

    draft.rotations.push(Rotation {
        id: Id::from_string(source.id.clone().unwrap_or_else(|| name.to_string())),
        name: name.to_string(),
        members,
        kind: kind_of_period(period),
        active_windows: minutes.ranges(),
        calendar: None,
        start_at: TimestampMicros(start),
        end_at: end.map(TimestampMicros),
    });

    Some(minutes)
}

fn members_of(draft: &mut Draft<'_>, source: &SourceRotation, name: &str) -> Vec<Id> {
    let mut members = Vec::new();

    for participant in &source.participants {
        let label = participant
            .name
            .as_deref()
            .or(participant.id.as_deref())
            .unwrap_or_default();

        match (participant.kind.as_str(), participant.username.as_deref()) {
            ("user", Some(email)) => members.push(draft.member(email)),
            ("user", None) => draft.block(
                Finding::new("user_without_email")
                    .with("rotation", name)
                    .with("user", label),
            ),
            ("team", _) => draft.block(
                Finding::new("participant_team")
                    .with("rotation", name)
                    .with("name", label),
            ),
            ("escalation", _) => draft.block(
                Finding::new("participant_escalation")
                    .with("rotation", name)
                    .with("name", label),
            ),
            ("none", _) => draft.block(Finding::new("participant_none").with("rotation", name)),
            (other, _) => draft.block(
                Finding::new("participant_unknown")
                    .with("rotation", name)
                    .with("value", other),
            ),
        }
    }

    members
}

/// 轮换的周期（秒）。
fn period_of(draft: &mut Draft<'_>, source: &SourceRotation, name: &str) -> Option<u64> {
    let unit: u64 = match source.kind.as_str() {
        "hourly" => 3_600,
        "daily" => 86_400,
        "weekly" => 604_800,
        other => {
            draft.block(
                Finding::new("rotation_type_unsupported")
                    .with("rotation", name)
                    .with("value", other),
            );
            return None;
        }
    };

    let length = source.length.unwrap_or(1);
    let period = u64::try_from(length)
        .ok()
        .filter(|length| *length > 0)
        .and_then(|length| length.checked_mul(unit));

    if period.is_none() {
        draft.block(
            Finding::new("rotation_length_invalid")
                .with("rotation", name)
                .with("value", length.to_string()),
        );
    }

    period
}
