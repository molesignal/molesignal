// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 升级步骤的负责人：步骤直接点名的用户，以及排班此刻的值班人。

use std::collections::HashMap;

use crate::{
    domain::alerting::{
        escalation::{EscalationStep, EscalationTarget},
        incident::Incident,
        repositories::ScheduleRepository,
        schedule::Schedule,
    },
    shared::{ids::Id, time::TimestampMicros},
};

/// 一轮 tick 内的排班缓存：同一个排班只读一次库。没有注入排班仓库时，排班目标不产生负责人。
pub(super) struct ScheduleCache<'a> {
    repo: Option<&'a dyn ScheduleRepository>,
    loaded: HashMap<Id, Option<Schedule>>,
}

impl<'a> ScheduleCache<'a> {
    pub(super) fn new(repo: Option<&'a dyn ScheduleRepository>) -> Self {
        Self {
            repo,
            loaded: HashMap::new(),
        }
    }

    async fn on_call(&mut self, schedule_id: &Id, at: TimestampMicros) -> Option<Id> {
        let repo = self.repo?;

        if !self.loaded.contains_key(schedule_id) {
            let schedule = match repo.get(schedule_id).await {
                Ok(schedule) => Some(schedule),
                Err(error) => {
                    tracing::warn!(
                        schedule_id = %schedule_id,
                        error = %error,
                        "escalation target schedule could not be loaded"
                    );
                    None
                }
            };

            self.loaded.insert(schedule_id.clone(), schedule);
        }

        self.loaded.get(schedule_id)?.as_ref()?.who_is_on_call(at)
    }
}

/// 把 `incident.assignees` 对齐到该步骤直接点名的人，返回是否有变化。
///
/// `user` 目标是本人，`schedule` 目标是此刻的值班人；`team` 目标是广播，不指定个人。
/// 步骤没有点名任何个人（只广播给团队、或值班表此刻没人）时保持现状。
pub(super) async fn apply_assignees(
    incident: &mut Incident,
    step: &EscalationStep,
    schedules: &mut ScheduleCache<'_>,
    at: TimestampMicros,
) -> bool {
    let mut desired: Vec<Id> = Vec::new();

    for target in &step.targets {
        let user = match target {
            EscalationTarget::User { user_id } => Some(user_id.clone()),
            EscalationTarget::Schedule { schedule_id } => schedules.on_call(schedule_id, at).await,
            EscalationTarget::Team { .. } => None,
        };

        if let Some(user) = user
            && !desired.contains(&user)
        {
            desired.push(user);
        }
    }

    if desired.is_empty() || incident.assignees == desired {
        return false;
    }

    incident.assignees = desired;
    true
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;

    use super::*;
    use crate::{
        domain::alerting::{
            incident::{IncidentStatus, Severity},
            schedule::{Rotation, RotationKind},
        },
        shared::{Error, Result},
    };

    struct Schedules(HashMap<Id, Schedule>);

    #[async_trait]
    impl ScheduleRepository for Schedules {
        async fn create(&self, schedule: Schedule) -> Result<Schedule> {
            Ok(schedule)
        }
        async fn update(&self, schedule: Schedule) -> Result<Schedule> {
            Ok(schedule)
        }
        async fn get(&self, id: &Id) -> Result<Schedule> {
            self.0
                .get(id)
                .cloned()
                .ok_or_else(|| Error::not_found("schedule"))
        }
        async fn list(&self, _org_id: &Id) -> Result<Vec<Schedule>> {
            Ok(self.0.values().cloned().collect())
        }
        async fn delete(&self, _id: &Id) -> Result<()> {
            Ok(())
        }
    }

    fn user(id: &str) -> EscalationTarget {
        EscalationTarget::User {
            user_id: Id::from_string(id),
        }
    }

    fn step(targets: Vec<EscalationTarget>) -> EscalationStep {
        EscalationStep {
            targets,
            ack_timeout_secs: 60,
            min_severity: None,
        }
    }

    fn on_call_schedule(id: &str, member: &str) -> Schedule {
        Schedule {
            id: Id::from_string(id),
            org_id: Id::from_string("org"),
            name: id.to_owned(),
            description: String::new(),
            team_id: None,
            timezone: "UTC".to_owned(),
            enabled: true,
            rotations: vec![Rotation {
                id: Id::from_string("rotation"),
                name: "primary".to_owned(),
                members: vec![Id::from_string(member)],
                kind: RotationKind::Daily,
                active_window: None,
                start_at: TimestampMicros(0),
            }],
            overrides: Vec::new(),
            created_by: None,
            updated_by: None,
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(0),
        }
    }

    fn incident(assignees: &[&str]) -> Incident {
        Incident {
            id: Id::from_string("incident"),
            org_id: Id::from_string("org"),
            rule_id: Id::from_string("rule"),
            escalation_policy_id: Id::from_string("policy"),
            status: IncidentStatus::Open,
            severity: Severity::Warning,
            summary: String::new(),
            fingerprint: String::new(),
            current_step: 0,
            current_loop: 0,
            current_step_started_at: TimestampMicros(0),
            assignees: assignees.iter().map(|id| Id::from_string(*id)).collect(),
            labels: Default::default(),
            annotations: Default::default(),
            trace_ids: Vec::new(),
            host_ids: Vec::new(),
            affected_services: Vec::new(),
            triggering_query: None,
            created_at: TimestampMicros(0),
            acknowledged_at: None,
            acknowledged_by: None,
            resolved_at: None,
            resolved_by: None,
        }
    }

    #[tokio::test]
    async fn users_and_on_call_schedule_become_assignees() {
        let repo = Schedules(HashMap::from([(
            Id::from_string("s1"),
            on_call_schedule("s1", "oncall"),
        )]));
        let mut cache = ScheduleCache::new(Some(&repo));
        let step = step(vec![
            user("alice"),
            EscalationTarget::Schedule {
                schedule_id: Id::from_string("s1"),
            },
            EscalationTarget::Team {
                team_id: Id::from_string("t1"),
            },
            user("alice"),
        ]);
        let mut incident = incident(&[]);

        assert!(apply_assignees(&mut incident, &step, &mut cache, TimestampMicros(1_000)).await);
        assert_eq!(
            incident.assignees,
            vec![Id::from_string("alice"), Id::from_string("oncall")]
        );
        assert!(!apply_assignees(&mut incident, &step, &mut cache, TimestampMicros(2_000)).await);
    }

    #[tokio::test]
    async fn team_only_step_keeps_existing_assignees() {
        let mut cache = ScheduleCache::new(None);
        let step = step(vec![EscalationTarget::Team {
            team_id: Id::from_string("t1"),
        }]);
        let mut incident = incident(&["alice"]);

        assert!(!apply_assignees(&mut incident, &step, &mut cache, TimestampMicros(1)).await);
        assert_eq!(incident.assignees, vec![Id::from_string("alice")]);
    }

    #[tokio::test]
    async fn next_step_replaces_the_previous_assignees() {
        let mut cache = ScheduleCache::new(None);
        let mut incident = incident(&["alice"]);

        assert!(
            apply_assignees(
                &mut incident,
                &step(vec![user("bob")]),
                &mut cache,
                TimestampMicros(1)
            )
            .await
        );
        assert_eq!(incident.assignees, vec![Id::from_string("bob")]);
    }
}
