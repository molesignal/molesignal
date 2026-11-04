// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! PagerDuty 的排班：`GET /schedules/{id}` 响应里的层（`schedule_layers`）换成轮值。
//!
//! - 层有重叠时最新的优先，API 把最新的层排在最前面；轮值也是排在前面的先算，顺序不动。优先的层在限制之外
//!   没人值班时，由后面的层接着，和 PagerDuty 的层一样。
//! - `rotation_virtual_start` 是轮换的起点，`start`、`end` 是这一层生效的起止。起点到 `start` 之间已经
//!   过去时没有影响；`start` 还没到时把轮值的起点挪到 `start`，人也跟着转到那一刻该值班的。
//! - 限制的时刻按请求的 `time_zone` 渲染。`GET /schedules/{id}` 不传时按排班自己的时区，`GET /schedules`
//!   不传时按调用者的时区，所以文件要用前者取；按别的时区渲染的认得出来，挡住。
//! - 响应里的用户只有 id 和名字，邮箱要从 `GET /users` 取，放在文件顶层的 `users` 里。
//! - shift-based 排班（V3 API）在 `schedules` 里，`type` 是 `schedule_v3`，转换见 [`shift_based`]。V2 的接口查不到
//!   它们，取数脚本把 `GET /v3/schedules` 的列表放在 `shift_based_schedules` 里；只有名字、没取到内容的明确跳过。

use std::collections::{HashMap, HashSet};

use chrono_tz::Tz;
use serde::Deserialize;
use serde_json::Value;

use self::{
    final_schedule::{SubSchedule, rendered_of},
    restrictions::{Restriction, restricted_minutes},
};
use super::{
    Draft, ForeignImport, ForeignSchedule, PAGERDUTY_FORMAT, kind_of_period, local_text,
    parse_time, rendered, weekly::WeekMinutes,
};
use crate::{
    app::alerting::oncall_transfer::{Directory, Finding, TransferItem},
    domain::alerting::schedule::Rotation,
    shared::{ids::Id, time::TimestampMicros},
};

mod final_schedule;
mod restrictions;
mod shift_based;

#[derive(Deserialize)]
struct Schedule {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    time_zone: Option<String>,
    #[serde(default)]
    teams: Vec<Reference>,
    #[serde(default)]
    schedule_layers: Option<Vec<Layer>>,
    #[serde(default)]
    final_schedule: Option<SubSchedule>,
    #[serde(default)]
    overrides_subschedule: Option<SubSchedule>,
}

impl Schedule {
    fn display_name(&self) -> Option<&str> {
        self.name
            .as_deref()
            .or(self.summary.as_deref())
            .or(self.id.as_deref())
    }
}

#[derive(Deserialize)]
struct Reference {
    id: String,
    #[serde(default)]
    summary: Option<String>,
}

#[derive(Deserialize)]
struct Layer {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    start: String,
    #[serde(default)]
    end: Option<String>,
    rotation_virtual_start: String,
    rotation_turn_length_seconds: i64,
    #[serde(default)]
    users: Vec<LayerUser>,
    #[serde(default)]
    restrictions: Vec<Restriction>,
}

#[derive(Deserialize)]
struct LayerUser {
    user: Reference,
}

/// 用户 id → 邮箱。
type Emails = HashMap<String, String>;

pub(super) fn recognizes(bundle: &Value) -> bool {
    bundle.get("schedule").is_some_and(Value::is_object)
        || bundle.get("schedules").is_some_and(Value::is_array)
        || bundle
            .get("shift_based_schedules")
            .is_some_and(Value::is_array)
}

pub(super) fn read(bundle: &Value, directory: &Directory, now: i64) -> ForeignImport {
    let emails = emails_of(bundle);
    let window = rendered_window(bundle);
    let single = bundle.get("schedule").into_iter();
    let many = bundle
        .get("schedules")
        .and_then(Value::as_array)
        .into_iter()
        .flatten();

    let mut schedules = Vec::new();
    let mut references = Vec::new();
    let mut converted = HashSet::new();

    for raw in single.chain(many) {
        match raw.get("type").and_then(Value::as_str) {
            Some("schedule_v3") => {
                converted.extend(id_of(raw));
                schedules.push(shift_based::read(raw, &emails, window, directory, now));
            }
            Some("schedule_v3_reference") => references.push(raw),
            _ => match serde_json::from_value::<Schedule>(raw.clone()) {
                Ok(schedule) => schedules.push(convert(&schedule, &emails, window, directory, now)),
                Err(error) => schedules.push(unreadable(raw, &error.to_string(), directory)),
            },
        }
    }

    let listed = bundle
        .get("shift_based_schedules")
        .and_then(Value::as_array);

    // 取到内容的已经转换了，同一个排班也只说一次。
    let left_out = references
        .into_iter()
        .chain(listed.into_iter().flatten())
        .filter(|raw| id_of(raw).is_none_or(|id| converted.insert(id)))
        .map(not_fetched)
        .collect();

    let mut findings = vec![Finding::new("schedules_only")];

    if listed.is_none() {
        findings.push(Finding::new("shift_based_not_listed"));
    }

    ForeignImport {
        format: PAGERDUTY_FORMAT,
        schedules,
        left_out,
        findings,
    }
}

fn emails_of(bundle: &Value) -> Emails {
    bundle
        .get("users")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|user| {
            Some((
                user.get("id")?.as_str()?.to_string(),
                user.get("email")?.as_str()?.to_string(),
            ))
        })
        .collect()
}

/// 取数脚本把渲染用的 `since`、`until` 放在文件顶层；PagerDuty 的响应本身不带。
fn rendered_window(bundle: &Value) -> Option<(i64, i64)> {
    let time = |key: &str| bundle.get(key).and_then(Value::as_str).and_then(parse_time);

    Some((time("since")?, time("until")?))
}

fn id_of(raw: &Value) -> Option<&str> {
    raw.get("id").and_then(Value::as_str)
}

/// 文件只说了有这个 shift-based 排班，没带它的内容。
fn not_fetched(raw: &Value) -> TransferItem {
    let name = ["summary", "name", "id"]
        .iter()
        .find_map(|key| raw.get(*key).and_then(Value::as_str));

    TransferItem {
        findings: vec![Finding::new("shift_based_not_fetched")],
        ..TransferItem::skipped(name.unwrap_or_default())
    }
}

fn unreadable(raw: &Value, error: &str, directory: &Directory) -> ForeignSchedule {
    let name = raw
        .get("name")
        .or_else(|| raw.get("summary"))
        .and_then(Value::as_str);
    let mut draft = Draft::new(directory, name, None);

    draft.block(Finding::new("schedule_unreadable").with("detail", error));
    draft.finish()
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
        schedule.display_name(),
        schedule.time_zone.as_deref(),
    );

    draft.description = schedule.description.clone().filter(|text| !text.is_empty());
    draft.adopt_team(
        schedule
            .teams
            .iter()
            .filter_map(|team| team.summary.as_deref()),
    );

    let Some(tz) = draft.zone() else {
        return draft.finish();
    };

    let Some(layers) = &schedule.schedule_layers else {
        draft.block(Finding::new("layers_missing"));
        return draft.finish();
    };

    for (index, layer) in layers.iter().enumerate() {
        if draft.rotations_full() {
            break;
        }

        let name = layer
            .name
            .clone()
            .unwrap_or_else(|| format!("Layer {}", layers.len() - index));

        add_layer(&mut draft, layer, &name, emails, tz, now);
    }

    let overrides = schedule
        .overrides_subschedule
        .as_ref()
        .map_or(
            &[][..],
            |overrides| &overrides.rendered_schedule_entries[..],
        );

    if draft.blockers.is_empty() {
        let rendered = rendered_of(schedule, overrides, emails, window);

        rendered::check(&mut draft, tz, rendered, now);
    }

    if !overrides.is_empty() {
        draft.note(Finding::new("overrides_left_out").with("count", overrides.len().to_string()));
    }

    draft.finish()
}

fn add_layer(draft: &mut Draft<'_>, layer: &Layer, name: &str, emails: &Emails, tz: Tz, now: i64) {
    let invalid = |value: &str| {
        Finding::new("time_invalid")
            .with("rotation", name)
            .with("value", value)
    };

    let Some(start) = parse_time(&layer.start) else {
        draft.block(invalid(&layer.start));
        return;
    };

    let Some(anchor) = parse_time(&layer.rotation_virtual_start) else {
        draft.block(invalid(&layer.rotation_virtual_start));
        return;
    };

    let end = match layer.end.as_deref() {
        None => None,
        Some(text) => match parse_time(text) {
            Some(end) => Some(end),
            None => {
                draft.block(invalid(text));
                return;
            }
        },
    };

    // 已经结束的层不影响现在往后谁值班，它的内容也就不用查了。
    if let Some(end) = end.filter(|end| *end <= now || *end <= start) {
        draft.note(
            Finding::new("rotation_ended")
                .with("rotation", name)
                .with("at", local_text(end, tz)),
        );
        return;
    }

    let before = draft.blockers.len();
    let members = members_of(draft, layer, name, emails);
    let minutes = restricted_minutes(draft, layer, name, tz);
    let period = u64::try_from(layer.rotation_turn_length_seconds)
        .ok()
        .filter(|seconds| *seconds > 0);

    if period.is_none() {
        draft.block(
            Finding::new("rotation_length_invalid")
                .with("rotation", name)
                .with("value", layer.rotation_turn_length_seconds.to_string()),
        );
    }

    let (Some(period), true) = (period, draft.blockers.len() == before) else {
        return;
    };

    // 没人的层、限制之内一分钟也不生效的层，不会有人值班。
    if members.is_empty() || minutes.as_ref().is_some_and(WeekMinutes::is_empty) {
        draft.note(Finding::new("rotation_empty").with("rotation", name));
        return;
    }

    let rotation = Rotation {
        id: Id::from_string(layer.id.clone().unwrap_or_else(|| name.to_string())),
        name: name.to_string(),
        members,
        kind: kind_of_period(period),
        active_windows: minutes
            .as_ref()
            .map(WeekMinutes::ranges)
            .unwrap_or_default(),
        calendar: None,
        start_at: TimestampMicros(anchor),
        end_at: end.map(TimestampMicros),
    };

    if start < anchor && anchor > now {
        draft.block(Finding::new("starts_before_handoff").with("rotation", name));
    } else if start > anchor && start > now {
        draft.rotations.extend(starting_later(rotation, start, tz));
    } else {
        draft.rotations.push(rotation);
    }
}

fn members_of(draft: &mut Draft<'_>, layer: &Layer, name: &str, emails: &Emails) -> Vec<Id> {
    let mut members = Vec::new();

    for LayerUser { user } in &layer.users {
        match emails.get(&user.id) {
            Some(email) => members.push(draft.member(email)),
            None => draft.block(
                Finding::new("user_without_email")
                    .with("rotation", name)
                    .with("user", user.summary.as_deref().unwrap_or(&user.id)),
            ),
        }
    }

    members
}

/// 层从 `start` 才生效，那时轮换早已开始：起点挪到 `start`，人转到那一刻该值班的。`start` 不是交接时刻时，
/// 到下一次交接前的那一段单独成一条，到点结束。
fn starting_later(rotation: Rotation, start: i64, tz: Tz) -> Vec<Rotation> {
    let Some(index) = rotation.shift_index(start, tz) else {
        return vec![rotation];
    };
    let count = rotation.members.len() as i64;
    let turn = index.rem_euclid(count) as usize;

    if rotation.handoff(index, tz) == start {
        let mut members = rotation.members.clone();

        members.rotate_left(turn);

        return vec![Rotation {
            members,
            start_at: TimestampMicros(start),
            ..rotation
        }];
    }

    let next = rotation.handoff(index + 1, tz);
    let ends_first = rotation.end_at.is_some_and(|end| end.0 <= next);

    let first = Rotation {
        id: Id::from_string(format!("{}-first", rotation.id.0)),
        members: vec![rotation.members[turn].clone()],
        start_at: TimestampMicros(start),
        end_at: Some(
            rotation
                .end_at
                .filter(|_| ends_first)
                .unwrap_or(TimestampMicros(next)),
        ),
        ..rotation.clone()
    };

    if ends_first {
        return vec![first];
    }

    let mut members = rotation.members.clone();

    members.rotate_left((turn + 1) % count as usize);

    vec![
        first,
        Rotation {
            members,
            start_at: TimestampMicros(next),
            ..rotation
        },
    ]
}
