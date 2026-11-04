// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 路由端到端（内存仓库，不连数据库）：规则开的事件把升级策略留给默认时，按路由规则、服务的
//! 负责团队、团队标签、默认策略的顺序定团队和策略；来源明确选的策略照旧，只定团队。改了团队的
//! 值班策略，下一个事件就用新的；路由规则写入前校验去向，可以重排；「试一试」说出每一步看到了什么。

use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex as StdMutex},
};

use async_trait::async_trait;
use molesignal::{
    app::alerting::{
        AlertRouting, RuleEvaluator,
        routing::{RoutingProbe, RuleWrite},
    },
    domain::{
        alerting::{
            escalation::{DEFAULT_POLICY_ID, EscalationPolicy},
            incident::{Incident, IncidentStatus, Severity},
            repositories::{
                AlertRuleEvalState, AlertRuleEvalStateRepository, AlertRuleRepository,
                EscalationPolicyRepository, IncidentRepository,
            },
            routing::{
                ConditionOperator, LabelCondition, RoutingReason, RoutingRule,
                RoutingRuleRepository, RoutingTarget, ServiceOwner, ServiceOwnerSource, TeamPolicy,
                TeamPolicyRepository,
            },
            rule::{AlertQuery, AlertRule, AlertRuleKind, AlertTrigger, ComparisonOp, RuleState},
        },
        iam::{Team, TeamRepository},
        query::{QueryEngine, QueryLanguage, QueryRequest, QueryResult},
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
        Ok(incident)
    }

    async fn update_if_status(
        &self,
        incident: Incident,
        _: &[IncidentStatus],
    ) -> Result<Option<Incident>> {
        Ok(Some(incident))
    }

    async fn list_merged_into(&self, _: &Id, _: &Id) -> Result<Vec<Incident>> {
        Ok(Vec::new())
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
        Ok(self.0.lock().unwrap().clone())
    }

    async fn list_active_for_rule(&self, _: &Id, rule_id: &Id) -> Result<Vec<Incident>> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .iter()
            .filter(|incident| &incident.rule_id == rule_id)
            .cloned()
            .collect())
    }

    async fn find_by_fingerprint(&self, _: &Id, fingerprint: &str) -> Result<Option<Incident>> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .iter()
            .find(|incident| incident.fingerprint == fingerprint)
            .cloned())
    }

    async fn list_by_status(&self, _: &Id, _: IncidentStatus) -> Result<Vec<Incident>> {
        Ok(Vec::new())
    }

    async fn list_since(&self, _: &Id, _: TimestampMicros) -> Result<Vec<Incident>> {
        Ok(Vec::new())
    }
}

#[derive(Default)]
struct Rules(StdMutex<Vec<RoutingRule>>);

#[async_trait]
impl RoutingRuleRepository for Rules {
    async fn create(&self, rule: RoutingRule) -> Result<RoutingRule> {
        self.upsert(rule).await
    }

    async fn update(&self, rule: RoutingRule) -> Result<RoutingRule> {
        self.upsert(rule).await
    }

    async fn upsert(&self, rule: RoutingRule) -> Result<RoutingRule> {
        let mut all = self.0.lock().unwrap();
        all.retain(|other| other.id != rule.id);
        all.push(rule.clone());
        Ok(rule)
    }

    async fn get(&self, id: &Id) -> Result<RoutingRule> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .find(|rule| &rule.id == id)
            .cloned()
            .ok_or_else(|| Error::not_found("routing rule"))
    }

    async fn list(&self, org_id: &Id) -> Result<Vec<RoutingRule>> {
        let mut rules: Vec<RoutingRule> = self
            .0
            .lock()
            .unwrap()
            .iter()
            .filter(|rule| &rule.org_id == org_id)
            .cloned()
            .collect();
        rules.sort_by_key(|rule| rule.position);
        Ok(rules)
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        self.0.lock().unwrap().retain(|rule| &rule.id != id);
        Ok(())
    }

    async fn reorder(&self, org_id: &Id, ids: &[Id]) -> Result<Vec<RoutingRule>> {
        let current = self.list(org_id).await?;
        let ordered: Vec<Id> = ids
            .iter()
            .filter(|id| current.iter().any(|rule| &rule.id == *id))
            .cloned()
            .chain(
                current
                    .iter()
                    .map(|rule| rule.id.clone())
                    .filter(|id| !ids.contains(id)),
            )
            .collect();

        for rule in self.0.lock().unwrap().iter_mut() {
            if let Some(position) = ordered.iter().position(|id| id == &rule.id) {
                rule.position = position as i32;
            }
        }

        self.list(org_id).await
    }
}

#[derive(Default)]
struct TeamPolicies(StdMutex<Vec<TeamPolicy>>);

#[async_trait]
impl TeamPolicyRepository for TeamPolicies {
    async fn list(&self, org_id: &Id) -> Result<Vec<TeamPolicy>> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .iter()
            .filter(|policy| &policy.org_id == org_id)
            .cloned()
            .collect())
    }

    async fn upsert(&self, policy: TeamPolicy) -> Result<TeamPolicy> {
        let mut all = self.0.lock().unwrap();
        all.retain(|other| !(other.org_id == policy.org_id && other.team_id == policy.team_id));
        all.push(policy.clone());
        Ok(policy)
    }

    async fn delete(&self, org_id: &Id, team_id: &Id) -> Result<()> {
        self.0
            .lock()
            .unwrap()
            .retain(|policy| !(&policy.org_id == org_id && &policy.team_id == team_id));
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

    async fn list(&self, org_id: &Id) -> Result<Vec<Team>> {
        Ok(self
            .0
            .iter()
            .filter(|team| &team.org_id == org_id)
            .cloned()
            .collect())
    }

    async fn delete(&self, _: &Id) -> Result<()> {
        Ok(())
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

    async fn list(&self, org_id: &Id) -> Result<Vec<EscalationPolicy>> {
        Ok(self
            .0
            .iter()
            .filter(|policy| &policy.org_id == org_id)
            .cloned()
            .collect())
    }

    async fn delete(&self, _: &Id) -> Result<()> {
        Ok(())
    }
}

struct Owners(Vec<ServiceOwner>);

#[async_trait]
impl ServiceOwnerSource for Owners {
    async fn owners(&self, _: &Id) -> Result<Vec<ServiceOwner>> {
        Ok(self.0.clone())
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

struct NoEvalState;

#[async_trait]
impl AlertRuleEvalStateRepository for NoEvalState {
    async fn claim_cycle(&self, _: &Id, _: TimestampMicros, _: i64) -> Result<bool> {
        Ok(true)
    }

    async fn upsert_state(&self, state: AlertRuleEvalState) -> Result<AlertRuleEvalState> {
        Ok(state)
    }

    async fn get(&self, _: &Id, _: &str) -> Result<Option<AlertRuleEvalState>> {
        Ok(None)
    }

    async fn list_groups(&self, _: &Id) -> Result<Vec<AlertRuleEvalState>> {
        Ok(Vec::new())
    }

    async fn reset(&self, _: &Id, _: &str) -> Result<()> {
        Ok(())
    }

    async fn forget_groups(&self, _: &Id, _: &[String]) -> Result<()> {
        Ok(())
    }
}

struct NoQueries;

#[async_trait]
impl QueryEngine for NoQueries {
    async fn execute(&self, _: QueryRequest) -> Result<QueryResult> {
        Err(Error::internal("routing runs no query"))
    }
}

// ---- 场景 ----

fn id(value: &str) -> Id {
    Id::from_string(value)
}

fn org() -> Id {
    id("org-a")
}

fn user() -> Id {
    id("user-1")
}

fn team(team_id: &str, org_id: &str, name: &str) -> Team {
    Team {
        id: id(team_id),
        org_id: id(org_id),
        name: name.into(),
        member_ids: vec![user()],
    }
}

fn policy(policy_id: &str, org_id: &str) -> EscalationPolicy {
    EscalationPolicy {
        id: id(policy_id),
        org_id: id(org_id),
        name: policy_id.into(),
        steps: Vec::new(),
        repeat: false,
        max_loops: 1,
        ack_retrigger_secs: None,
        urgency_rule: Default::default(),
        escalate_low_urgency: false,
    }
}

struct World {
    routing: Arc<AlertRouting>,
    evaluator: RuleEvaluator,
}

/// payments 的值班策略是 pol-pay，search 没设；checkout 归 payments。org-b 有自己的团队和策略。
fn world() -> World {
    let team_policies = TeamPolicies::default();
    team_policies.0.lock().unwrap().push(TeamPolicy {
        org_id: org(),
        team_id: id("payments"),
        escalation_policy_id: id("pol-pay"),
        updated_by: None,
        updated_at: TimestampMicros(0),
    });

    let routing = Arc::new(AlertRouting::new(
        Arc::new(Rules::default()),
        Arc::new(team_policies),
        Arc::new(Teams(vec![
            team("payments", "org-a", "Payments"),
            team("search", "org-a", "Search"),
            team("other-team", "org-b", "Other"),
        ])),
        Arc::new(Policies(vec![
            policy("pol-pay", "org-a"),
            policy("pol-db", "org-a"),
            policy("pol-night", "org-a"),
            policy("pol-1", "org-a"),
            policy("pol-other", "org-b"),
        ])),
        Some(Arc::new(Owners(vec![ServiceOwner {
            namespace: "default".into(),
            name: "checkout".into(),
            team_id: id("payments"),
        }]))),
    ));

    let evaluator = RuleEvaluator::new(
        Arc::new(NoRules),
        Arc::new(Incidents::default()),
        Arc::new(NoEvalState),
        Arc::new(NoQueries),
        5,
    )
    .with_router(routing.router());

    World { routing, evaluator }
}

fn rule(rule_id: &str, policy: &str, labels: &[(&str, &str)]) -> AlertRule {
    AlertRule {
        id: id(rule_id),
        org_id: org(),
        name: rule_id.into(),
        description: String::new(),
        enabled: true,
        kind: AlertRuleKind::Scheduled,
        query: AlertQuery {
            language: QueryLanguage::Sql,
            statement: "SELECT count(*) FROM logs".into(),
            period_secs: 60,
            stream: None,
            apm: None,
            rum: None,
            group_by: Vec::new(),
            composite: None,
            forecast: None,
            outlier: None,
            heartbeat: None,
            delay_secs: None,
            discovery: None,
            home_cluster: None,
        },
        anomaly_params: None,
        trigger: AlertTrigger {
            operator: ComparisonOp::Gt,
            threshold: 1.0,
            for_periods: 1,
            silence_secs: 0,
            no_data: None,
            recovery: None,
            priority: None,
            urgency: None,
        },
        thresholds: Vec::new(),
        severity: Some(Severity::Error),
        escalation_policy_id: id(policy),
        labels: labels
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect(),
        annotations: BTreeMap::new(),
        last_eval_at: None,
        last_state: RuleState::default(),
        created_at: TimestampMicros(0),
        updated_at: TimestampMicros(0),
    }
}

impl World {
    async fn trigger(&self, rule: AlertRule) -> Incident {
        self.evaluator
            .trigger_rule(&rule, &user(), TimestampMicros::now())
            .await
            .unwrap()
    }
}

fn write(name: &str, conditions: Vec<LabelCondition>, target: RoutingTarget) -> RuleWrite {
    RuleWrite {
        name: name.into(),
        enabled: true,
        conditions,
        min_severity: None,
        origins: Vec::new(),
        time: None,
        priorities: Vec::new(),
        target,
    }
}

fn equals(key: &str, value: &str) -> LabelCondition {
    LabelCondition {
        key: key.into(),
        operator: ConditionOperator::Equals,
        values: vec![value.into()],
    }
}

#[tokio::test]
async fn an_incident_left_to_the_default_policy_follows_rules_then_owner_then_team_label() {
    let world = world();

    let owned = world
        .trigger(rule(
            "checkout-errors",
            DEFAULT_POLICY_ID,
            &[("service", "checkout")],
        ))
        .await;
    assert_eq!(owned.escalation_policy_id, id("pol-pay"));
    assert_eq!(owned.team_id, Some(id("payments")));
    assert_eq!(
        owned.routing.as_ref().unwrap().reason,
        RoutingReason::ServiceOwner
    );
    assert_eq!(
        owned.routing.as_ref().unwrap().service.as_deref(),
        Some("default/checkout")
    );

    // Search has no on-call policy: the team is told, the default policy pages.
    let labelled = world
        .trigger(rule(
            "search-slow",
            DEFAULT_POLICY_ID,
            &[("team", "search")],
        ))
        .await;
    assert_eq!(labelled.escalation_policy_id, id(DEFAULT_POLICY_ID));
    assert_eq!(labelled.team_id, Some(id("search")));
    assert_eq!(
        labelled.routing.as_ref().unwrap().reason,
        RoutingReason::TeamLabel
    );

    let nobody = world
        .trigger(rule("disk-full", DEFAULT_POLICY_ID, &[("host", "web-1")]))
        .await;
    assert_eq!(nobody.escalation_policy_id, id(DEFAULT_POLICY_ID));
    assert_eq!(nobody.team_id, None);
    assert_eq!(
        nobody.routing.as_ref().unwrap().reason,
        RoutingReason::Default
    );

    // A routing rule comes first; the team still follows the service.
    world
        .routing
        .create_rule(
            &org(),
            &user(),
            write(
                "databases",
                vec![equals("db.system", "postgresql")],
                RoutingTarget::Policy {
                    policy_id: id("pol-db"),
                },
            ),
        )
        .await
        .unwrap();

    let database = world
        .trigger(rule(
            "checkout-db",
            DEFAULT_POLICY_ID,
            &[("db.system", "postgresql"), ("service", "checkout")],
        ))
        .await;
    assert_eq!(database.escalation_policy_id, id("pol-db"));
    assert_eq!(database.team_id, Some(id("payments")));
    assert_eq!(
        database.routing.as_ref().unwrap().rule_name.as_deref(),
        Some("databases")
    );

    // A policy the rule chose stays; only the team is found.
    let chosen = world
        .trigger(rule(
            "checkout-latency",
            "pol-1",
            &[("service", "checkout")],
        ))
        .await;
    assert_eq!(chosen.escalation_policy_id, id("pol-1"));
    assert_eq!(chosen.team_id, Some(id("payments")));
    assert_eq!(
        chosen.routing.as_ref().unwrap().reason,
        RoutingReason::Source
    );
}

#[tokio::test]
async fn a_changed_team_policy_is_used_by_the_next_incident() {
    let world = world();

    let before = world
        .trigger(rule("search-1", DEFAULT_POLICY_ID, &[("team", "search")]))
        .await;
    assert_eq!(before.escalation_policy_id, id(DEFAULT_POLICY_ID));

    world
        .routing
        .set_team_policy(&org(), &user(), &id("search"), Some(id("pol-night")))
        .await
        .unwrap();

    let after = world
        .trigger(rule("search-2", DEFAULT_POLICY_ID, &[("team", "search")]))
        .await;
    assert_eq!(after.escalation_policy_id, id("pol-night"));

    world
        .routing
        .set_team_policy(&org(), &user(), &id("search"), None)
        .await
        .unwrap();

    let removed = world
        .trigger(rule("search-3", DEFAULT_POLICY_ID, &[("team", "search")]))
        .await;
    assert_eq!(removed.escalation_policy_id, id(DEFAULT_POLICY_ID));
}

#[tokio::test]
async fn routing_rules_are_checked_scoped_and_kept_in_order() {
    let world = world();
    let routing = &world.routing;

    let unknown_policy = routing
        .create_rule(
            &org(),
            &user(),
            write(
                "x",
                Vec::new(),
                RoutingTarget::Policy {
                    policy_id: id("pol-other"),
                },
            ),
        )
        .await;
    assert!(matches!(unknown_policy, Err(Error::InvalidArgument(_))));

    let foreign_team = routing
        .create_rule(
            &org(),
            &user(),
            write(
                "x",
                Vec::new(),
                RoutingTarget::Team {
                    team_id: id("other-team"),
                },
            ),
        )
        .await;
    assert!(matches!(foreign_team, Err(Error::InvalidArgument(_))));

    assert!(
        routing
            .set_team_policy(&org(), &user(), &id("payments"), Some(id("pol-other")))
            .await
            .is_err()
    );

    let mut created = Vec::new();

    for name in ["first", "second", "third"] {
        created.push(
            routing
                .create_rule(
                    &org(),
                    &user(),
                    write(
                        name,
                        Vec::new(),
                        RoutingTarget::Team {
                            team_id: id("search"),
                        },
                    ),
                )
                .await
                .unwrap(),
        );
    }

    let reordered = routing
        .reorder(&org(), &[created[2].id.clone(), created[0].id.clone()])
        .await
        .unwrap();
    let names: Vec<&str> = reordered.iter().map(|rule| rule.name.as_str()).collect();
    assert_eq!(names, ["third", "first", "second"]);

    // Another organization's rules are not there to change or delete.
    let elsewhere = routing.delete_rule(&id("org-b"), &created[0].id).await;
    assert!(matches!(elsewhere, Err(Error::NotFound(_))));
}

#[tokio::test]
async fn the_dry_run_and_the_team_list_say_what_routing_sees() {
    let world = world();

    let explanation = world
        .routing
        .explain(
            &org(),
            RoutingProbe {
                labels: [
                    ("service.name".to_string(), "checkout".to_string()),
                    ("team".to_string(), "search".to_string()),
                ]
                .into(),
                services: Vec::new(),
                severity: Severity::Critical,
                priority: None,
                at: None,
            },
        )
        .await
        .unwrap();

    assert_eq!(explanation.service.as_deref(), Some("default/checkout"));
    assert_eq!(explanation.service_team_id, Some(id("payments")));
    assert_eq!(explanation.label_team_id, Some(id("search")));
    assert_eq!(explanation.decision.team_id, Some(id("payments")));
    assert_eq!(explanation.decision.escalation_policy_id, id("pol-pay"));

    let teams = world.routing.teams(&org()).await.unwrap();
    let rows: Vec<(&str, Option<&str>, usize)> = teams
        .iter()
        .map(|row| {
            (
                row.name.as_str(),
                row.escalation_policy_id
                    .as_ref()
                    .map(|policy| policy.0.as_str()),
                row.services.len(),
            )
        })
        .collect();
    assert_eq!(
        rows,
        [("Payments", Some("pol-pay"), 1), ("Search", None, 0)]
    );
}
