// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 人对事件的处理（内存仓库，不连数据库）：转派给成员、团队、升级策略，增派，立即升级，暂缓，
//! 改严重度，合并，备注与批量处理；每一步写回的样子和记下的时间线。

use std::sync::{Arc, Mutex as StdMutex};

use async_trait::async_trait;
use molesignal::{
    app::alerting::{
        Actor, AlertRouting, AlertingService, IncidentActions,
        actions::{BulkAction, Reassignment},
    },
    domain::{
        alerting::{
            escalation::{EscalationPolicy, EscalationStep, EscalationTarget},
            handling::EscalationReason,
            incident::{Incident, IncidentStatus, Severity},
            repositories::{
                AlertRuleRepository, EscalationPolicyRepository, IncidentRepository,
                ScheduleRepository,
            },
            routing::{RoutingRule, RoutingRuleRepository, TeamPolicy, TeamPolicyRepository},
            rule::{AlertRule, RuleState},
            schedule::Schedule,
            timeline::{ActionVia, IncidentTimelineRepository, TimelineEntry, TimelineKind},
        },
        iam::{IamAssignedRole, IamMembership, IamMembershipRepository, Team, TeamRepository},
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

// ---- 内存仓库 ----

#[derive(Default)]
struct Incidents(StdMutex<Vec<Incident>>);

#[async_trait]
impl IncidentRepository for Incidents {
    async fn create(&self, incident: Incident) -> Result<Incident> {
        self.0.lock().unwrap().push(incident.clone());
        Ok(incident)
    }

    async fn update(&self, incident: Incident) -> Result<Incident> {
        let mut all = self.0.lock().unwrap();
        let slot = all
            .iter_mut()
            .find(|other| other.id == incident.id)
            .ok_or_else(|| Error::not_found("incident"))?;
        let written = Incident {
            revision: slot.revision + 1,
            ..incident
        };
        *slot = written.clone();
        Ok(written)
    }

    async fn update_if_status(
        &self,
        incident: Incident,
        expected: &[IncidentStatus],
    ) -> Result<Option<Incident>> {
        let mut all = self.0.lock().unwrap();

        match all.iter_mut().find(|other| other.id == incident.id) {
            Some(slot) if expected.contains(&slot.status) && slot.revision == incident.revision => {
                let written = Incident {
                    revision: incident.revision + 1,
                    ..incident
                };
                *slot = written.clone();
                Ok(Some(written))
            }
            _ => Ok(None),
        }
    }

    async fn get(&self, id: &Id) -> Result<Incident> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .find(|incident| &incident.id == id)
            .cloned()
            .ok_or_else(|| Error::not_found("incident"))
    }

    async fn list_active(&self, _: &Id) -> Result<Vec<Incident>> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .iter()
            .filter(|incident| incident.is_active())
            .cloned()
            .collect())
    }

    async fn list_merged_into(&self, _: &Id, target_id: &Id) -> Result<Vec<Incident>> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .iter()
            .filter(|incident| incident.handling.merged_into.as_ref() == Some(target_id))
            .cloned()
            .collect())
    }

    async fn list_active_for_rule(&self, _: &Id, _: &Id) -> Result<Vec<Incident>> {
        Ok(Vec::new())
    }

    async fn find_by_fingerprint(&self, _: &Id, _: &str) -> Result<Option<Incident>> {
        Ok(None)
    }

    async fn list_by_status(&self, _: &Id, _: IncidentStatus) -> Result<Vec<Incident>> {
        Ok(Vec::new())
    }

    async fn list_since(&self, _: &Id, _: TimestampMicros) -> Result<Vec<Incident>> {
        Ok(Vec::new())
    }
}

#[derive(Default)]
struct Timeline(StdMutex<Vec<TimelineEntry>>);

#[async_trait]
impl IncidentTimelineRepository for Timeline {
    async fn append(&self, entry: TimelineEntry) -> Result<TimelineEntry> {
        self.0.lock().unwrap().push(entry.clone());
        Ok(entry)
    }

    async fn list(&self, _: &Id, incident_id: &Id) -> Result<Vec<TimelineEntry>> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .iter()
            .filter(|entry| &entry.incident_id == incident_id)
            .cloned()
            .collect())
    }

    async fn recent_notes(
        &self,
        org_id: &Id,
        incident_id: &Id,
        limit: u32,
    ) -> Result<Vec<TimelineEntry>> {
        let mut notes: Vec<TimelineEntry> = self
            .list(org_id, incident_id)
            .await?
            .into_iter()
            .filter(|entry| entry.kind == TimelineKind::Note)
            .collect();
        notes.reverse();
        notes.truncate(limit as usize);
        Ok(notes)
    }
}

struct Policies(Vec<EscalationPolicy>);

#[async_trait]
impl EscalationPolicyRepository for Policies {
    async fn create(&self, policy: EscalationPolicy) -> Result<EscalationPolicy> {
        Ok(policy)
    }

    async fn update(&self, policy: EscalationPolicy) -> Result<EscalationPolicy> {
        Ok(policy)
    }

    async fn get(&self, id: &Id) -> Result<EscalationPolicy> {
        self.0
            .iter()
            .find(|policy| &policy.id == id)
            .cloned()
            .ok_or_else(|| Error::not_found("policy"))
    }

    async fn list(&self, _: &Id) -> Result<Vec<EscalationPolicy>> {
        Ok(self.0.clone())
    }

    async fn delete(&self, _: &Id) -> Result<()> {
        Ok(())
    }
}

struct Teams(Vec<Team>);

#[async_trait]
impl TeamRepository for Teams {
    async fn create(&self, team: Team) -> Result<Team> {
        Ok(team)
    }

    async fn update(&self, team: Team) -> Result<Team> {
        Ok(team)
    }

    async fn get(&self, id: &Id) -> Result<Team> {
        self.0
            .iter()
            .find(|team| &team.id == id)
            .cloned()
            .ok_or_else(|| Error::not_found("team"))
    }

    async fn list(&self, _: &Id) -> Result<Vec<Team>> {
        Ok(self.0.clone())
    }

    async fn delete(&self, _: &Id) -> Result<()> {
        Ok(())
    }
}

struct TeamPolicies(Vec<TeamPolicy>);

#[async_trait]
impl TeamPolicyRepository for TeamPolicies {
    async fn list(&self, _: &Id) -> Result<Vec<TeamPolicy>> {
        Ok(self.0.clone())
    }

    async fn upsert(&self, policy: TeamPolicy) -> Result<TeamPolicy> {
        Ok(policy)
    }

    async fn delete(&self, _: &Id, _: &Id) -> Result<()> {
        Ok(())
    }
}

struct NoRoutingRules;

#[async_trait]
impl RoutingRuleRepository for NoRoutingRules {
    async fn create(&self, rule: RoutingRule) -> Result<RoutingRule> {
        Ok(rule)
    }

    async fn update(&self, rule: RoutingRule) -> Result<RoutingRule> {
        Ok(rule)
    }

    async fn upsert(&self, rule: RoutingRule) -> Result<RoutingRule> {
        Ok(rule)
    }

    async fn get(&self, _: &Id) -> Result<RoutingRule> {
        Err(Error::not_found("routing rule"))
    }

    async fn list(&self, _: &Id) -> Result<Vec<RoutingRule>> {
        Ok(Vec::new())
    }

    async fn delete(&self, _: &Id) -> Result<()> {
        Ok(())
    }

    async fn reorder(&self, _: &Id, _: &[Id]) -> Result<Vec<RoutingRule>> {
        Ok(Vec::new())
    }
}

struct Members(Vec<Id>);

#[async_trait]
impl IamMembershipRepository for Members {
    async fn upsert(&self, _: IamMembership, _: &[Id], _: &Id) -> Result<()> {
        Ok(())
    }

    async fn list_for_user(&self, _: &Id) -> Result<Vec<IamMembership>> {
        Ok(Vec::new())
    }

    async fn list_for_org(&self, org_id: &Id) -> Result<Vec<IamMembership>> {
        Ok(self
            .0
            .iter()
            .map(|user_id| IamMembership {
                user_id: user_id.clone(),
                org_id: org_id.clone(),
                joined_at: TimestampMicros(0),
            })
            .collect())
    }

    async fn assigned_roles(&self, _: &Id, _: &Id) -> Result<Vec<IamAssignedRole>> {
        Ok(Vec::new())
    }

    async fn role_id_for_purpose(&self, _: &Id, _: &str) -> Result<Id> {
        Err(Error::not_found("role"))
    }

    async fn remove(&self, _: &Id, _: &Id) -> Result<()> {
        Ok(())
    }
}

struct NoRules;

#[async_trait]
impl AlertRuleRepository for NoRules {
    async fn create(&self, rule: AlertRule) -> Result<AlertRule> {
        Ok(rule)
    }

    async fn update(&self, rule: AlertRule) -> Result<AlertRule> {
        Ok(rule)
    }

    async fn get(&self, _: &Id) -> Result<AlertRule> {
        Err(Error::not_found("rule"))
    }

    async fn list(&self, _: &Id) -> Result<Vec<AlertRule>> {
        Ok(Vec::new())
    }

    async fn delete(&self, _: &Id) -> Result<()> {
        Ok(())
    }

    async fn list_enabled(&self) -> Result<Vec<AlertRule>> {
        Ok(Vec::new())
    }

    async fn record_evaluation(&self, _: &Id, _: TimestampMicros, _: RuleState) -> Result<()> {
        Ok(())
    }
}

struct NoSchedules;

#[async_trait]
impl ScheduleRepository for NoSchedules {
    async fn create(&self, schedule: Schedule) -> Result<Schedule> {
        Ok(schedule)
    }

    async fn update(&self, schedule: Schedule) -> Result<Schedule> {
        Ok(schedule)
    }

    async fn get(&self, _: &Id) -> Result<Schedule> {
        Err(Error::not_found("schedule"))
    }

    async fn list(&self, _: &Id) -> Result<Vec<Schedule>> {
        Ok(Vec::new())
    }

    async fn delete(&self, _: &Id) -> Result<()> {
        Ok(())
    }
}

// ---- 场景 ----

fn id(value: &str) -> Id {
    Id::from_string(value)
}

fn org() -> Id {
    id("org-a")
}

fn step(users: &[&str], min_severity: Option<Severity>) -> EscalationStep {
    EscalationStep {
        targets: users
            .iter()
            .map(|user| EscalationTarget::User { user_id: id(user) })
            .collect(),
        ack_timeout_secs: 300,
        min_severity,
    }
}

fn policy(policy_id: &str, steps: Vec<EscalationStep>) -> EscalationPolicy {
    EscalationPolicy {
        id: id(policy_id),
        org_id: org(),
        name: policy_id.into(),
        steps,
        repeat: false,
        max_loops: 1,
        ack_retrigger_secs: None,
        urgency_rule: Default::default(),
        escalate_low_urgency: false,
    }
}

fn incident(incident_id: &str, status: IncidentStatus) -> Incident {
    Incident {
        id: id(incident_id),
        org_id: org(),
        rule_id: id("rule-1"),
        escalation_policy_id: id("pol-main"),
        status,
        severity: Severity::Warning,
        summary: format!("{incident_id} is failing"),
        fingerprint: format!("fp-{incident_id}"),
        current_step: 0,
        current_loop: 0,
        current_step_started_at: TimestampMicros(0),
        assignees: vec![id("u1")],
        labels: Default::default(),
        annotations: Default::default(),
        trace_ids: Vec::new(),
        host_ids: Vec::new(),
        affected_services: Vec::new(),
        triggering_query: None,
        created_at: TimestampMicros(0),
        acknowledged_at: (status == IncidentStatus::Acknowledged).then_some(TimestampMicros(1)),
        acknowledged_by: (status == IncidentStatus::Acknowledged).then(|| id("u1")),
        resolved_at: None,
        resolved_by: None,
        team_id: None,
        routing: None,
        handling: Default::default(),
        revision: 0,
    }
}

struct World {
    incidents: Arc<Incidents>,
    timeline: Arc<Timeline>,
    actions: IncidentActions,
}

/// pol-main：第一步 u1，第二步 u2（只管 error 以上），第三步 u3。payments 团队的值班策略是
/// pol-pay（一步，u4）。组织里有 u1–u4。
fn world(incidents: Vec<Incident>) -> World {
    let incidents = Arc::new(Incidents(StdMutex::new(incidents)));
    let timeline = Arc::new(Timeline::default());

    let policies = Arc::new(Policies(vec![
        policy(
            "pol-main",
            vec![
                step(&["u1"], None),
                step(&["u2"], Some(Severity::Error)),
                step(&["u3"], None),
            ],
        ),
        policy("pol-pay", vec![step(&["u4"], None)]),
    ]));

    let routing = Arc::new(AlertRouting::new(
        Arc::new(NoRoutingRules),
        Arc::new(TeamPolicies(vec![TeamPolicy {
            org_id: org(),
            team_id: id("payments"),
            escalation_policy_id: id("pol-pay"),
            updated_by: None,
            updated_at: TimestampMicros(0),
        }])),
        Arc::new(Teams(vec![Team {
            id: id("payments"),
            org_id: org(),
            name: "Payments".into(),
            member_ids: vec![id("u4")],
        }])),
        policies.clone(),
        None,
    ));

    let service = Arc::new(
        AlertingService::new(
            Arc::new(NoRules),
            incidents.clone(),
            Arc::new(NoSchedules),
            policies.clone(),
        )
        .with_timeline(timeline.clone()),
    );

    let actions = IncidentActions::new(
        incidents.clone(),
        timeline.clone(),
        policies,
        Arc::new(Members(vec![id("u1"), id("u2"), id("u3"), id("u4")])),
        routing,
        service,
    );

    World {
        incidents,
        timeline,
        actions,
    }
}

fn actor(user: &str) -> Actor {
    Actor {
        org_id: org(),
        user_id: id(user),
        via: ActionVia::Web,
        at: TimestampMicros(1_000_000),
    }
}

impl World {
    async fn incident(&self, incident_id: &str) -> Incident {
        self.incidents.get(&id(incident_id)).await.unwrap()
    }

    fn kinds(&self, incident_id: &str) -> Vec<TimelineKind> {
        self.timeline
            .0
            .lock()
            .unwrap()
            .iter()
            .filter(|entry| entry.incident_id == id(incident_id))
            .map(|entry| entry.kind)
            .collect()
    }
}

#[tokio::test]
async fn reassigning_to_people_reopens_the_incident_and_holds_them_until_the_next_step() {
    let world = world(vec![incident("inc-1", IncidentStatus::Acknowledged)]);

    let reassigned = world
        .actions
        .reassign(
            &actor("u1"),
            &id("inc-1"),
            Reassignment::Users {
                user_ids: vec![id("u2"), id("u2"), id("u3")],
            },
        )
        .await
        .unwrap();

    assert_eq!(reassigned.status, IncidentStatus::Open);
    assert_eq!(reassigned.acknowledged_by, None);
    assert_eq!(reassigned.assignees, vec![id("u2"), id("u3")]);
    assert!(reassigned.handling.assignees_pinned);
    assert_eq!(
        reassigned.current_step_started_at,
        TimestampMicros(1_000_000)
    );
    assert_eq!(world.kinds("inc-1"), vec![TimelineKind::Reassigned]);

    // 组织外的人不能指定。
    let error = world
        .actions
        .reassign(
            &actor("u1"),
            &id("inc-1"),
            Reassignment::Users {
                user_ids: vec![id("stranger")],
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(error, Error::InvalidArgument(_)), "{error:?}");
}

#[tokio::test]
async fn reassigning_to_a_team_restarts_its_on_call_policy_in_a_new_round() {
    let world = world(vec![incident("inc-1", IncidentStatus::Open)]);

    let reassigned = world
        .actions
        .reassign(
            &actor("u1"),
            &id("inc-1"),
            Reassignment::Team {
                team_id: id("payments"),
            },
        )
        .await
        .unwrap();

    assert_eq!(reassigned.team_id, Some(id("payments")));
    assert_eq!(reassigned.escalation_policy_id, id("pol-pay"));
    assert_eq!(reassigned.current_step, 0);
    assert!(reassigned.assignees.is_empty());
    assert_eq!(reassigned.handling.dispatch_round, 1);
    assert_eq!(
        reassigned.handling.dispatch_reason,
        EscalationReason::Reassigned
    );

    let to_policy = world
        .actions
        .reassign(
            &actor("u1"),
            &id("inc-1"),
            Reassignment::Policy {
                escalation_policy_id: id("pol-main"),
            },
        )
        .await
        .unwrap();

    // 换策略不换团队。
    assert_eq!(to_policy.team_id, Some(id("payments")));
    assert_eq!(to_policy.escalation_policy_id, id("pol-main"));
    assert_eq!(to_policy.handling.dispatch_round, 2);

    let missing = world
        .actions
        .reassign(
            &actor("u1"),
            &id("inc-1"),
            Reassignment::Policy {
                escalation_policy_id: id("pol-gone"),
            },
        )
        .await;
    assert!(missing.is_err());
}

#[tokio::test]
async fn adding_responders_keeps_the_status_and_skips_people_already_there() {
    let world = world(vec![incident("inc-1", IncidentStatus::Acknowledged)]);

    let added = world
        .actions
        .add_responders(&actor("u1"), &id("inc-1"), &[id("u1"), id("u2")])
        .await
        .unwrap();

    assert_eq!(added.status, IncidentStatus::Acknowledged);
    assert_eq!(added.assignees, vec![id("u1"), id("u2")]);

    let entry = world.timeline.0.lock().unwrap()[0].clone();
    assert_eq!(entry.kind, TimelineKind::RespondersAdded);
    assert_eq!(entry.detail["userIds"], serde_json::json!(["u2"]));

    // 都已经是负责人：什么也不做，不记时间线。
    world
        .actions
        .add_responders(&actor("u1"), &id("inc-1"), &[id("u2")])
        .await
        .unwrap();
    assert_eq!(world.kinds("inc-1").len(), 1);
}

#[tokio::test]
async fn escalating_skips_steps_that_do_not_apply_and_starts_a_round() {
    let world = world(vec![incident("inc-1", IncidentStatus::Acknowledged)]);

    // warning 级：第二步只管 error 以上，下一步是第三步。
    let escalated = world
        .actions
        .escalate(&actor("u1"), &id("inc-1"), None)
        .await
        .unwrap();

    assert_eq!(escalated.status, IncidentStatus::Open);
    assert_eq!(escalated.current_step, 2);
    assert_eq!(escalated.handling.dispatch_round, 1);
    assert_eq!(escalated.handling.dispatch_reason, EscalationReason::Manual);

    // 已经在最后一步：要指定一步；指定回第一步可以再叫一遍。
    assert!(
        world
            .actions
            .escalate(&actor("u1"), &id("inc-1"), None)
            .await
            .is_err()
    );
    assert!(
        world
            .actions
            .escalate(&actor("u1"), &id("inc-1"), Some(1))
            .await
            .is_err(),
        "the second step does not apply to a warning"
    );

    let again = world
        .actions
        .escalate(&actor("u1"), &id("inc-1"), Some(0))
        .await
        .unwrap();
    assert_eq!(again.current_step, 0);
    assert_eq!(again.handling.dispatch_round, 2);
}

#[tokio::test]
async fn snoozing_an_open_incident_acknowledges_it_first() {
    let world = world(vec![incident("inc-1", IncidentStatus::Open)]);

    let snoozed = world
        .actions
        .snooze(&actor("u2"), &id("inc-1"), 3600)
        .await
        .unwrap();

    assert_eq!(snoozed.status, IncidentStatus::Acknowledged);
    assert_eq!(snoozed.acknowledged_by, Some(id("u2")));
    assert_eq!(
        snoozed.handling.snoozed_until,
        Some(TimestampMicros(1_000_000 + 3600 * 1_000_000))
    );
    assert_eq!(
        world.kinds("inc-1"),
        vec![TimelineKind::Acknowledged, TimelineKind::Snoozed]
    );

    assert!(
        world
            .actions
            .snooze(&actor("u2"), &id("inc-1"), 10)
            .await
            .is_err(),
        "a snooze lasts at least a minute"
    );

    let cancelled = world
        .actions
        .cancel_snooze(&actor("u2"), &id("inc-1"))
        .await
        .unwrap();
    assert_eq!(cancelled.handling.snoozed_until, None);
    assert_eq!(cancelled.status, IncidentStatus::Acknowledged);
}

#[tokio::test]
async fn a_severity_set_by_hand_is_kept_and_a_raise_restarts_the_open_incident() {
    let mut open = incident("inc-1", IncidentStatus::Open);
    open.current_step = 2;
    let world = world(vec![open]);

    let raised = world
        .actions
        .set_severity(&actor("u1"), &id("inc-1"), Severity::Critical)
        .await
        .unwrap();

    assert_eq!(raised.severity, Severity::Critical);
    assert_eq!(raised.handling.severity_set_by, Some(id("u1")));
    assert_eq!(raised.current_step, 0);
    assert_eq!(
        raised.handling.dispatch_reason,
        EscalationReason::SeverityRaised
    );

    let lowered = world
        .actions
        .set_severity(&actor("u1"), &id("inc-1"), Severity::Info)
        .await
        .unwrap();
    assert_eq!(lowered.severity, Severity::Info);
    assert_eq!(
        lowered.handling.dispatch_round, 1,
        "lowering starts no round"
    );
    assert_eq!(
        world.kinds("inc-1"),
        vec![TimelineKind::SeverityChanged, TimelineKind::SeverityChanged]
    );
}

#[tokio::test]
async fn merged_incidents_hang_under_the_target_and_resolve_with_it() {
    let mut nested = incident("inc-3", IncidentStatus::Open);
    nested.handling.merged_into = Some(id("inc-2"));

    let world = world(vec![
        incident("inc-1", IncidentStatus::Open),
        incident("inc-2", IncidentStatus::Acknowledged),
        nested,
    ]);

    let target = world
        .actions
        .merge(&actor("u1"), &id("inc-1"), &[id("inc-2"), id("inc-1")])
        .await
        .unwrap();
    assert_eq!(target.id, id("inc-1"));

    // inc-2 并入 inc-1；原来并入 inc-2 的 inc-3 改挂到 inc-1 下。
    for merged in ["inc-2", "inc-3"] {
        let merged = world.incident(merged).await;
        assert_eq!(merged.handling.merged_into, Some(id("inc-1")));
        assert!(merged.is_active());
    }

    assert_eq!(world.kinds("inc-1"), vec![TimelineKind::MergedIn]);

    // 并入的事件不能直接处理；主事件不能并入它自己并入的事件。
    assert!(
        world
            .actions
            .snooze(&actor("u1"), &id("inc-2"), 3600)
            .await
            .is_err()
    );
    assert!(
        world
            .actions
            .merge(&actor("u1"), &id("inc-2"), &[id("inc-1")])
            .await
            .is_err()
    );

    let timeline = world
        .actions
        .timeline(&actor("u1"), &id("inc-1"))
        .await
        .unwrap();
    assert_eq!(timeline.merged.len(), 2);

    let service = AlertingService::new(
        Arc::new(NoRules),
        world.incidents.clone(),
        Arc::new(NoSchedules),
        Arc::new(Policies(Vec::new())),
    );
    service
        .resolve(&id("inc-1"), id("u1"), TimestampMicros(9), ActionVia::Web)
        .await
        .unwrap();

    for id_ in ["inc-1", "inc-2", "inc-3"] {
        assert_eq!(
            world.incident(id_).await.status,
            IncidentStatus::Resolved,
            "{id_}"
        );
    }
}

#[tokio::test]
async fn notes_are_trimmed_and_bulk_reports_every_incident() {
    let mut resolved = incident("inc-2", IncidentStatus::Resolved);
    resolved.resolved_at = Some(TimestampMicros(5));
    let world = world(vec![incident("inc-1", IncidentStatus::Open), resolved]);

    let note = world
        .actions
        .note(&actor("u1"), &id("inc-2"), "  回滚了 v2.3  ")
        .await
        .unwrap();
    assert_eq!(note.detail["text"], "回滚了 v2.3");
    assert!(
        world
            .actions
            .note(&actor("u1"), &id("inc-2"), "   ")
            .await
            .is_err()
    );

    let notes = world
        .actions
        .recent_notes(&actor("u1"), &id("inc-2"), 5)
        .await
        .unwrap();
    assert_eq!(notes.len(), 1);

    let results = world
        .actions
        .bulk(
            &actor("u1"),
            &[id("inc-1"), id("inc-2")],
            &BulkAction::Snooze { duration_secs: 600 },
        )
        .await;

    assert_eq!(results.len(), 2);
    assert!(results[0].error.is_none());
    assert!(
        results[1].error.is_some(),
        "a resolved incident cannot be snoozed"
    );
    assert_eq!(
        world.incident("inc-1").await.status,
        IncidentStatus::Acknowledged
    );
}
