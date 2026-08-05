// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::{collections::HashSet, sync::Arc};

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;

use crate::{
    domain::{
        alerting::{repositories::ScheduleRepository, schedule::Schedule},
        notify::{
            policy::NotifyEvent,
            recipient::{NotifyRecipient, RecipientResolver},
        },
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub const NEXT_ONCALL_RESOLVER: &str = "next_oncall";
pub const SCHEDULE_MEMBERS_RESOLVER: &str = "schedule_members";

#[derive(Debug, Deserialize)]
struct ScheduleConfig {
    schedule_id: Id,
}

fn parse_config(config: &Value, resolver: &str) -> Result<ScheduleConfig> {
    serde_json::from_value(config.clone())
        .map_err(|error| Error::invalid(format!("invalid {resolver} config: {error}")))
}

async fn get_schedule(
    schedules: &dyn ScheduleRepository,
    event: &NotifyEvent,
    id: &Id,
) -> Result<Schedule> {
    schedules
        .list(&event.organization_id)
        .await?
        .into_iter()
        .find(|schedule| schedule.id == *id)
        .ok_or_else(|| Error::not_found("on-call schedule"))
}

pub struct NextOncallResolver {
    schedules: Arc<dyn ScheduleRepository>,
}

impl NextOncallResolver {
    pub fn new(schedules: Arc<dyn ScheduleRepository>) -> Self {
        Self { schedules }
    }
}

#[async_trait]
impl RecipientResolver for NextOncallResolver {
    fn resolver_type(&self) -> &'static str {
        NEXT_ONCALL_RESOLVER
    }

    fn validate_config(&self, config: &Value) -> Result<()> {
        parse_config(config, NEXT_ONCALL_RESOLVER).map(|_| ())
    }

    async fn resolve(&self, event: &NotifyEvent, config: &Value) -> Result<Vec<NotifyRecipient>> {
        let config = parse_config(config, NEXT_ONCALL_RESOLVER)?;
        let schedule = get_schedule(self.schedules.as_ref(), event, &config.schedule_id).await?;
        let user_id = event
            .attributes
            .get("next_user_id")
            .or_else(|| event.attributes.get("nextUserId"))
            .and_then(Value::as_str)
            .map(Id::from_string)
            .filter(|user_id| schedule_user_ids(&schedule).contains(user_id))
            .or_else(|| next_oncall_user(&schedule, event.occurred_at));
        Ok(user_id
            .map(|user_id| {
                vec![NotifyRecipient {
                    user_id,
                    team_id: schedule.team_id,
                }]
            })
            .unwrap_or_default())
    }
}

pub struct ScheduleMembersResolver {
    schedules: Arc<dyn ScheduleRepository>,
}

impl ScheduleMembersResolver {
    pub fn new(schedules: Arc<dyn ScheduleRepository>) -> Self {
        Self { schedules }
    }
}

#[async_trait]
impl RecipientResolver for ScheduleMembersResolver {
    fn resolver_type(&self) -> &'static str {
        SCHEDULE_MEMBERS_RESOLVER
    }

    fn validate_config(&self, config: &Value) -> Result<()> {
        parse_config(config, SCHEDULE_MEMBERS_RESOLVER).map(|_| ())
    }

    async fn resolve(&self, event: &NotifyEvent, config: &Value) -> Result<Vec<NotifyRecipient>> {
        let config = parse_config(config, SCHEDULE_MEMBERS_RESOLVER)?;
        let schedule = get_schedule(self.schedules.as_ref(), event, &config.schedule_id).await?;
        let team_id = schedule.team_id.clone();
        Ok(schedule_user_ids(&schedule)
            .into_iter()
            .map(|user_id| NotifyRecipient {
                user_id,
                team_id: team_id.clone(),
            })
            .collect())
    }
}

fn schedule_user_ids(schedule: &Schedule) -> HashSet<Id> {
    schedule
        .rotations
        .iter()
        .flat_map(|rotation| rotation.members.iter().cloned())
        .chain(schedule.overrides.iter().map(|value| value.user_id.clone()))
        .collect()
}

fn next_oncall_user(schedule: &Schedule, at: TimestampMicros) -> Option<Id> {
    let current = schedule.who_is_on_call(at);
    // 所有现有 rotation 最长为周级；扫描 370 天覆盖一年轮换，五分钟粒度足够
    // 识别交接，再用一分钟步长收窄边界。
    const STEP: i64 = 5 * 60 * 1_000_000;
    const HORIZON: i64 = 370 * 24 * 60 * 60 * 1_000_000;
    let mut previous_at = at.0;
    let mut offset = STEP;
    while offset <= HORIZON {
        let candidate_at = TimestampMicros(at.0.saturating_add(offset));
        let candidate = schedule.who_is_on_call(candidate_at);
        if candidate.is_some() && candidate != current {
            let mut probe = previous_at.saturating_add(60 * 1_000_000);
            while probe <= candidate_at.0 {
                let resolved = schedule.who_is_on_call(TimestampMicros(probe));
                if resolved.is_some() && resolved != current {
                    return resolved;
                }
                probe = probe.saturating_add(60 * 1_000_000);
            }
            return candidate;
        }
        previous_at = candidate_at.0;
        offset = offset.saturating_add(STEP);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::alerting::schedule::{Rotation, RotationKind};

    #[test]
    fn next_oncall_finds_next_rotation_member() {
        let schedule = Schedule {
            id: Id::from_string("schedule"),
            org_id: Id::from_string("org"),
            name: "Primary".into(),
            description: String::new(),
            team_id: None,
            timezone: "UTC".into(),
            enabled: true,
            rotations: vec![Rotation {
                id: Id::from_string("rotation"),
                name: "daily".into(),
                members: vec![Id::from_string("a"), Id::from_string("b")],
                kind: RotationKind::Daily,
                active_window: None,
                start_at: TimestampMicros(0),
            }],
            overrides: Vec::new(),
            created_by: None,
            updated_by: None,
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(0),
        };
        assert_eq!(
            next_oncall_user(&schedule, TimestampMicros(1)),
            Some(Id::from_string("b"))
        );
    }
}
