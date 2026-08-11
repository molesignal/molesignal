// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::{collections::BTreeMap, sync::Arc};

use serde_json::{Map, Value};

use crate::{
    app::notify::{NotifyDispatch, NotifyEngine},
    domain::{
        alerting::{
            repositories::ScheduleRepository,
            schedule::{Schedule, ScheduleOverride},
        },
        notify::{connector::NotifyMessage, policy::NotifyEvent},
    },
    shared::{Result, ids::Id, time::TimestampMicros},
};

pub const ONCALL_SHIFT_STARTING_EVENT: &str = "oncall.shift.starting";
pub const ONCALL_SHIFT_STARTED_EVENT: &str = "oncall.shift.started";
pub const ONCALL_OVERRIDE_CREATED_EVENT: &str = "oncall.override.created";
pub const ONCALL_COVERAGE_MISSING_EVENT: &str = "oncall.coverage.missing";

const MINUTE_MICROS: i64 = 60 * 1_000_000;
const STARTING_WINDOW_MINUTES: i64 = 30;
const STARTED_LOOKBACK_MINUTES: i64 = 5;

pub struct OncallEventProducer {
    engine: Arc<NotifyEngine>,
    schedules: Arc<dyn ScheduleRepository>,
}

impl OncallEventProducer {
    pub fn new(engine: Arc<NotifyEngine>, schedules: Arc<dyn ScheduleRepository>) -> Self {
        Self { engine, schedules }
    }

    pub async fn tick(&self, organization_id: &Id, now: TimestampMicros) -> Result<()> {
        for schedule in self
            .schedules
            .list(organization_id)
            .await?
            .into_iter()
            .filter(|schedule| schedule.enabled)
        {
            for dispatch in schedule_dispatches(&schedule, now) {
                if let Err(error) = self.engine.enqueue_event(dispatch).await {
                    tracing::warn!(
                        org_id = %organization_id,
                        schedule_id = %schedule.id,
                        error = %error,
                        "on-call notify event enqueue failed"
                    );
                }
            }
        }
        Ok(())
    }
}

pub fn override_created_dispatch(
    schedule: &Schedule,
    schedule_override: &ScheduleOverride,
) -> NotifyDispatch {
    let mut without_override = schedule.clone();
    without_override
        .overrides
        .retain(|value| value.id != schedule_override.id);
    let original_user = without_override.who_is_on_call(schedule_override.start_at);
    let user_ids = original_user
        .iter()
        .chain(std::iter::once(&schedule_override.user_id))
        .cloned()
        .collect::<Vec<_>>();
    let mut attributes = schedule_attributes(schedule, user_ids);
    attributes.insert(
        "override_id".into(),
        Value::String(schedule_override.id.to_string()),
    );
    attributes.insert(
        "override_user_id".into(),
        Value::String(schedule_override.user_id.to_string()),
    );
    attributes.insert(
        "start_at_micros".into(),
        Value::Number(schedule_override.start_at.0.into()),
    );
    attributes.insert(
        "end_at_micros".into(),
        Value::Number(schedule_override.end_at.0.into()),
    );
    attributes.insert(
        "reason".into(),
        Value::String(schedule_override.reason.clone()),
    );
    if let Some(user_id) = &original_user {
        attributes.insert(
            "original_user_id".into(),
            Value::String(user_id.to_string()),
        );
    }
    attributes.insert(
        "override".into(),
        serde_json::json!({
            "id": schedule_override.id.to_string(),
            "user_id": schedule_override.user_id.to_string(),
            "original_user_id": original_user.as_ref().map(ToString::to_string),
            "start_at": schedule_override.start_at.0,
            "end_at": schedule_override.end_at.0,
            "reason": schedule_override.reason
        }),
    );
    dispatch(
        schedule,
        ONCALL_OVERRIDE_CREATED_EVENT,
        format!(
            "{ONCALL_OVERRIDE_CREATED_EVENT}:{}:{}",
            schedule.id, schedule_override.id
        ),
        TimestampMicros::now(),
        attributes,
        "On-call override created",
        format!("{} has a temporary on-call override.", schedule.name),
    )
}

fn schedule_dispatches(schedule: &Schedule, now: TimestampMicros) -> Vec<NotifyDispatch> {
    let now = floor_minute(now);
    let mut dispatches = Vec::new();
    for minute in 0..STARTED_LOOKBACK_MINUTES {
        let at = TimestampMicros(now.0.saturating_sub(minute * MINUTE_MICROS));
        let before = TimestampMicros(at.0.saturating_sub(MINUTE_MICROS));
        if let (Some(previous), Some(current)) =
            (schedule.who_is_on_call(before), schedule.who_is_on_call(at))
            && previous != current
        {
            dispatches.push(shift_dispatch(
                schedule,
                ONCALL_SHIFT_STARTED_EVENT,
                at,
                previous,
                current,
            ));
        }
    }

    if let Some((transition_at, current, next)) = next_transition(schedule, now) {
        dispatches.push(shift_dispatch(
            schedule,
            ONCALL_SHIFT_STARTING_EVENT,
            transition_at,
            current,
            next,
        ));
    }

    if schedule.who_is_on_call(now).is_none() {
        let hour_bucket = now.0 / (60 * MINUTE_MICROS);
        let attributes = schedule_attributes(schedule, manager_user_ids(schedule));
        dispatches.push(dispatch(
            schedule,
            ONCALL_COVERAGE_MISSING_EVENT,
            format!(
                "{ONCALL_COVERAGE_MISSING_EVENT}:{}:{hour_bucket}",
                schedule.id
            ),
            now,
            attributes,
            "On-call coverage missing",
            format!("{} currently has no on-call coverage.", schedule.name),
        ));
    }
    dispatches
}

fn next_transition(schedule: &Schedule, now: TimestampMicros) -> Option<(TimestampMicros, Id, Id)> {
    let current = schedule.who_is_on_call(now)?;
    for minute in 1..=STARTING_WINDOW_MINUTES + 1 {
        let at = TimestampMicros(now.0.saturating_add(minute * MINUTE_MICROS));
        if let Some(next) = schedule.who_is_on_call(at)
            && next != current
        {
            return Some((at, current, next));
        }
    }
    None
}

fn shift_dispatch(
    schedule: &Schedule,
    event_type: &str,
    transition_at: TimestampMicros,
    previous: Id,
    current: Id,
) -> NotifyDispatch {
    let mut attributes = schedule_attributes(schedule, vec![previous.clone(), current.clone()]);
    attributes.insert(
        "current_user_id".into(),
        Value::String(previous.to_string()),
    );
    attributes.insert("next_user_id".into(), Value::String(current.to_string()));
    attributes.insert(
        "transition_at_micros".into(),
        Value::Number(transition_at.0.into()),
    );
    attributes.insert(
        "oncall".into(),
        serde_json::json!({
            "current_user_id": previous.to_string(),
            "next_user_id": current.to_string(),
            "transition_at": transition_at.0
        }),
    );
    let title = if event_type == ONCALL_SHIFT_STARTED_EVENT {
        "On-call shift started"
    } else {
        "On-call shift starting soon"
    };
    dispatch(
        schedule,
        event_type,
        format!("{event_type}:{}:{}", schedule.id, transition_at.0),
        if event_type == ONCALL_SHIFT_STARTED_EVENT {
            transition_at
        } else {
            TimestampMicros(
                transition_at
                    .0
                    .saturating_sub(STARTING_WINDOW_MINUTES * MINUTE_MICROS),
            )
        },
        attributes,
        title,
        format!("{} changes on-call coverage.", schedule.name),
    )
}

fn schedule_attributes(schedule: &Schedule, user_ids: Vec<Id>) -> Map<String, Value> {
    let mut attributes = Map::new();
    attributes.insert("schedule_id".into(), Value::String(schedule.id.to_string()));
    attributes.insert("schedule_name".into(), Value::String(schedule.name.clone()));
    attributes.insert("timezone".into(), Value::String(schedule.timezone.clone()));
    attributes.insert(
        "user_ids".into(),
        Value::Array(
            user_ids
                .into_iter()
                .map(|id| Value::String(id.to_string()))
                .collect(),
        ),
    );
    attributes.insert(
        "manager_user_ids".into(),
        Value::Array(
            manager_user_ids(schedule)
                .into_iter()
                .map(|id| Value::String(id.to_string()))
                .collect(),
        ),
    );
    if let Some(team_id) = &schedule.team_id {
        attributes.insert("team_id".into(), Value::String(team_id.to_string()));
    }
    attributes.insert(
        "schedule".into(),
        serde_json::json!({
            "id": schedule.id.to_string(),
            "name": schedule.name,
            "team_id": schedule.team_id.as_ref().map(ToString::to_string),
            "timezone": schedule.timezone
        }),
    );
    attributes
}

fn manager_user_ids(schedule: &Schedule) -> Vec<Id> {
    schedule
        .updated_by
        .iter()
        .chain(schedule.created_by.iter())
        .cloned()
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn dispatch(
    schedule: &Schedule,
    event_type: &str,
    id: String,
    occurred_at: TimestampMicros,
    attributes: Map<String, Value>,
    title: &str,
    text: String,
) -> NotifyDispatch {
    let mut metadata = BTreeMap::new();
    metadata.insert("schedule_id".into(), schedule.id.to_string());
    metadata.insert("event_type".into(), event_type.into());
    NotifyDispatch {
        event: NotifyEvent {
            id,
            event_type: event_type.into(),
            organization_id: schedule.org_id.clone(),
            occurred_at,
            attributes: Value::Object(attributes),
        },
        message: NotifyMessage {
            title: title.into(),
            text: text.clone(),
            markdown: Some(format!("**{title}**\n\n{text}")),
            html: None,
            metadata,
        },
    }
}

fn floor_minute(value: TimestampMicros) -> TimestampMicros {
    TimestampMicros(value.0 - value.0.rem_euclid(MINUTE_MICROS))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::alerting::schedule::{Rotation, RotationKind};

    fn schedule() -> Schedule {
        Schedule {
            id: Id::from_string("schedule"),
            org_id: Id::from_string("org"),
            name: "Primary".into(),
            description: String::new(),
            team_id: Some(Id::from_string("team")),
            timezone: "UTC".into(),
            enabled: true,
            rotations: vec![Rotation {
                id: Id::from_string("rotation"),
                name: "minute".into(),
                members: vec![Id::from_string("a"), Id::from_string("b")],
                kind: RotationKind::Custom { period_secs: 60 },
                active_window: None,
                start_at: TimestampMicros(0),
            }],
            overrides: Vec::new(),
            created_by: Some(Id::from_string("admin")),
            updated_by: None,
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(0),
        }
    }

    #[test]
    fn transition_event_ids_are_stable() {
        let first = schedule_dispatches(&schedule(), TimestampMicros(31_000_000));
        let second = schedule_dispatches(&schedule(), TimestampMicros(59_000_000));
        let first_starting = first
            .iter()
            .find(|value| value.event.event_type == ONCALL_SHIFT_STARTING_EVENT)
            .expect("starting event");
        let second_starting = second
            .iter()
            .find(|value| value.event.event_type == ONCALL_SHIFT_STARTING_EVENT)
            .expect("starting event");
        assert_eq!(first_starting.event.id, second_starting.event.id);
        assert_eq!(
            first_starting.event.attributes["schedule"]["name"],
            "Primary"
        );
        assert_eq!(
            first_starting.event.attributes["schedule"]["timezone"],
            "UTC"
        );
        assert_eq!(
            first_starting.event.attributes["oncall"]["current_user_id"],
            "a"
        );
        assert_eq!(
            first_starting.event.attributes["oncall"]["next_user_id"],
            "b"
        );
    }

    #[test]
    fn override_event_contains_template_fields() {
        let schedule = schedule();
        let dispatch = override_created_dispatch(
            &schedule,
            &ScheduleOverride {
                id: Id::from_string("override"),
                user_id: Id::from_string("substitute"),
                start_at: TimestampMicros(0),
                end_at: TimestampMicros(60_000_000),
                reason: "handoff".into(),
            },
        );
        assert_eq!(dispatch.event.attributes["schedule"]["id"], "schedule");
        assert_eq!(dispatch.event.attributes["override"]["id"], "override");
        assert_eq!(
            dispatch.event.attributes["override"]["user_id"],
            "substitute"
        );
        assert_eq!(
            dispatch.event.attributes["override"]["original_user_id"],
            "a"
        );
        assert_eq!(dispatch.event.attributes["override"]["reason"], "handoff");
    }
}
