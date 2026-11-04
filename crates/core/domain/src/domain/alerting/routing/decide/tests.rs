// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::BTreeMap;

use super::*;
use crate::{
    domain::alerting::{
        incident::{EXTERNAL_SOURCE, SOURCE_LABEL},
        routing::{IncidentOrigin, RoutingRule, RoutingTarget, TeamPolicy},
    },
    shared::time::TimestampMicros,
};

fn id(value: &str) -> Id {
    Id::from_string(value)
}

fn labels(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect()
}

fn condition(key: &str, operator: ConditionOperator, values: &[&str]) -> LabelCondition {
    LabelCondition {
        key: key.into(),
        operator,
        values: values.iter().map(|value| value.to_string()).collect(),
    }
}

fn rule(
    name: &str,
    position: i32,
    conditions: Vec<LabelCondition>,
    target: RoutingTarget,
) -> RoutingRule {
    RoutingRule {
        id: id(name),
        org_id: id("org"),
        name: name.into(),
        enabled: true,
        position,
        conditions,
        min_severity: None,
        origins: Vec::new(),
        time: None,
        priorities: Vec::new(),
        target,
        created_by: None,
        created_at: TimestampMicros(0),
        updated_at: TimestampMicros(0),
    }
}

fn team_policy(team: &str, policy: &str) -> TeamPolicy {
    TeamPolicy {
        org_id: id("org"),
        team_id: id(team),
        escalation_policy_id: id(policy),
        updated_by: None,
        updated_at: TimestampMicros(0),
    }
}

fn owner(namespace: &str, name: &str, team: &str) -> ServiceOwner {
    ServiceOwner {
        namespace: namespace.into(),
        name: name.into(),
        team_id: id(team),
    }
}

/// payments 设了值班策略 pol-pay，search 没设；checkout 归 payments，search-api 归 search。
fn table(rules: Vec<RoutingRule>) -> RoutingTable {
    RoutingTable::new(
        rules,
        vec![team_policy("payments", "pol-pay")],
        vec![
            owner("default", "checkout", "payments"),
            owner("shop", "search-api", "search"),
        ],
        vec![
            (id("payments"), "Payments".into()),
            (id("search"), "Search".into()),
            (id("dup-a"), "Ops".into()),
            (id("dup-b"), "ops".into()),
        ],
        vec![id("pol-pay"), id("pol-db"), id("pol-night")],
    )
}

fn decide(
    table: &RoutingTable,
    labels: &BTreeMap<String, String>,
    severity: Severity,
) -> RoutingDecision {
    table.decide(&RoutingSubject {
        priority: None,
        at: TimestampMicros(0),
        labels,
        affected_services: &[],
        severity,
        source_policy: &id(DEFAULT_POLICY_ID),
    })
}

#[test]
fn the_first_matching_rule_wins_and_disabled_rules_do_not_count() {
    let mut off = rule(
        "off",
        0,
        vec![condition("env", ConditionOperator::Equals, &["prod"])],
        RoutingTarget::Policy {
            policy_id: id("pol-night"),
        },
    );
    off.enabled = false;

    let table = table(vec![
        rule(
            "later",
            20,
            vec![condition(
                "env",
                ConditionOperator::In,
                &["prod", "staging"],
            )],
            RoutingTarget::Policy {
                policy_id: id("pol-night"),
            },
        ),
        off,
        rule(
            "databases",
            10,
            vec![condition(
                "db.system",
                ConditionOperator::Matches,
                &["postgres(ql)?"],
            )],
            RoutingTarget::Policy {
                policy_id: id("pol-db"),
            },
        ),
    ]);

    let decision = decide(
        &table,
        &labels(&[("env", "prod"), ("db.system", "postgresql")]),
        Severity::Error,
    );

    assert_eq!(decision.escalation_policy_id, id("pol-db"));
    assert_eq!(decision.routing.reason, RoutingReason::Rule);
    assert_eq!(decision.routing.rule_name.as_deref(), Some("databases"));

    // The pattern matches the whole value only.
    let decision = decide(
        &table,
        &labels(&[("env", "prod"), ("db.system", "postgres-x")]),
        Severity::Error,
    );
    assert_eq!(decision.routing.rule_name.as_deref(), Some("later"));
}

#[test]
fn a_rule_can_be_limited_by_severity_and_origin() {
    let mut critical = rule(
        "critical-external",
        0,
        Vec::new(),
        RoutingTarget::Policy {
            policy_id: id("pol-night"),
        },
    );
    critical.min_severity = Some(Severity::Critical);
    critical.origins = vec![IncidentOrigin::External];

    let table = table(vec![critical]);
    let external = labels(&[(SOURCE_LABEL, EXTERNAL_SOURCE)]);

    assert_eq!(
        decide(&table, &external, Severity::Critical).escalation_policy_id,
        id("pol-night")
    );
    assert_eq!(
        decide(&table, &external, Severity::Error).routing.reason,
        RoutingReason::Default
    );
    assert_eq!(
        decide(&table, &labels(&[]), Severity::Critical)
            .routing
            .reason,
        RoutingReason::Default
    );
}

#[test]
fn a_team_target_takes_the_team_policy_or_the_default_one() {
    let table = table(vec![
        rule(
            "to-search",
            0,
            vec![condition("area", ConditionOperator::Equals, &["search"])],
            RoutingTarget::Team {
                team_id: id("search"),
            },
        ),
        rule(
            "to-payments",
            1,
            vec![condition("area", ConditionOperator::Equals, &["pay"])],
            RoutingTarget::Team {
                team_id: id("payments"),
            },
        ),
    ]);

    let paid = decide(&table, &labels(&[("area", "pay")]), Severity::Warning);
    assert_eq!(paid.escalation_policy_id, id("pol-pay"));
    assert_eq!(paid.team_id, Some(id("payments")));

    // Search has no on-call policy: the team is still told, the default policy pages.
    let searched = decide(&table, &labels(&[("area", "search")]), Severity::Warning);
    assert_eq!(searched.escalation_policy_id, id(DEFAULT_POLICY_ID));
    assert_eq!(searched.team_id, Some(id("search")));
    assert_eq!(searched.routing.reason, RoutingReason::Rule);
}

#[test]
fn a_rule_whose_target_is_gone_is_passed_over() {
    let table = table(vec![
        rule(
            "gone-policy",
            0,
            Vec::new(),
            RoutingTarget::Policy {
                policy_id: id("deleted"),
            },
        ),
        rule(
            "gone-team",
            1,
            Vec::new(),
            RoutingTarget::Team {
                team_id: id("deleted"),
            },
        ),
    ]);

    assert_eq!(
        decide(&table, &labels(&[]), Severity::Info).routing.reason,
        RoutingReason::Default
    );
}

#[test]
fn the_service_owner_comes_next_by_label_or_affected_service() {
    let table = table(Vec::new());

    let by_label = decide(
        &table,
        &labels(&[("service.name", "checkout")]),
        Severity::Error,
    );
    assert_eq!(by_label.escalation_policy_id, id("pol-pay"));
    assert_eq!(by_label.team_id, Some(id("payments")));
    assert_eq!(by_label.routing.reason, RoutingReason::ServiceOwner);
    assert_eq!(
        by_label.routing.service.as_deref(),
        Some("default/checkout")
    );

    let namespaced = decide(
        &table,
        &labels(&[("service", "search-api"), ("service.namespace", "shop")]),
        Severity::Error,
    );
    assert_eq!(namespaced.team_id, Some(id("search")));
    assert_eq!(namespaced.escalation_policy_id, id(DEFAULT_POLICY_ID));

    let affected = table.decide(&RoutingSubject {
        priority: None,
        at: TimestampMicros(0),
        labels: &labels(&[]),
        affected_services: &["checkout".to_string()],
        severity: Severity::Error,
        source_policy: &id(DEFAULT_POLICY_ID),
    });
    assert_eq!(affected.team_id, Some(id("payments")));
}

#[test]
fn a_team_label_names_a_team_by_name_or_id_unless_the_name_is_shared() {
    let table = table(Vec::new());

    let by_name = decide(&table, &labels(&[("team", " PAYMENTS ")]), Severity::Error);
    assert_eq!(by_name.team_id, Some(id("payments")));
    assert_eq!(by_name.routing.reason, RoutingReason::TeamLabel);
    assert_eq!(by_name.escalation_policy_id, id("pol-pay"));

    let by_id = decide(&table, &labels(&[("team_id", "search")]), Severity::Error);
    assert_eq!(by_id.team_id, Some(id("search")));

    let shared = decide(&table, &labels(&[("team", "ops")]), Severity::Error);
    assert_eq!(shared.team_id, None);
    assert_eq!(shared.routing.reason, RoutingReason::Default);

    // The service owner comes before the label.
    let both = decide(
        &table,
        &labels(&[("team", "search"), ("service.name", "checkout")]),
        Severity::Error,
    );
    assert_eq!(both.team_id, Some(id("payments")));
}

#[test]
fn a_policy_the_source_chose_stays_and_only_the_team_is_found() {
    let table = table(vec![rule(
        "all",
        0,
        Vec::new(),
        RoutingTarget::Policy {
            policy_id: id("pol-night"),
        },
    )]);
    let checkout = labels(&[("service.name", "checkout")]);

    for chosen in ["pol-db", ""] {
        let decision = table.decide(&RoutingSubject {
            priority: None,
            at: TimestampMicros(0),
            labels: &checkout,
            affected_services: &[],
            severity: Severity::Critical,
            source_policy: &id(chosen),
        });

        assert_eq!(decision.escalation_policy_id, id(chosen));
        assert_eq!(decision.team_id, Some(id("payments")));
        assert_eq!(decision.routing.reason, RoutingReason::Source);
        assert_eq!(decision.routing.rule_id, None);
    }
}

#[test]
fn the_explanation_says_what_each_step_saw() {
    let table = table(Vec::new());
    let explanation = table.explain(&RoutingSubject {
        priority: None,
        at: TimestampMicros(0),
        labels: &labels(&[("service.name", "checkout"), ("team", "search")]),
        affected_services: &[],
        severity: Severity::Error,
        source_policy: &id(DEFAULT_POLICY_ID),
    });

    assert_eq!(explanation.service.as_deref(), Some("default/checkout"));
    assert_eq!(explanation.service_team_id, Some(id("payments")));
    assert_eq!(explanation.label_team_id, Some(id("search")));
    assert_eq!(explanation.matched_rule_id, None);
}

#[test]
fn conditions_are_checked_before_they_are_kept() {
    let mut bad = rule(
        "bad",
        0,
        vec![condition("env", ConditionOperator::Matches, &["(unclosed"])],
        RoutingTarget::Policy {
            policy_id: id("pol-db"),
        },
    );
    assert!(bad.validate().is_err());

    bad.conditions = vec![condition("env", ConditionOperator::Equals, &["a", "b"])];
    assert!(bad.validate().is_err());

    bad.conditions = vec![condition("env", ConditionOperator::In, &[])];
    assert!(bad.validate().is_err());

    bad.conditions = vec![condition("env", ConditionOperator::NotEquals, &["dev"])];
    assert!(bad.validate().is_ok());
}

#[test]
fn a_rule_can_hold_only_at_some_times_and_for_some_priorities() {
    use crate::domain::alerting::{
        priority::Priority,
        time_window::{ClockTime, TimeCondition, TimeConditionMode, WeeklyRange, WeeklyWindows},
    };

    let mut nights = rule(
        "nights",
        0,
        Vec::new(),
        RoutingTarget::Policy {
            policy_id: id("pol-night"),
        },
    );
    nights.time = Some(TimeCondition {
        mode: TimeConditionMode::Outside,
        windows: WeeklyWindows {
            timezone: "UTC".into(),
            ranges: vec![WeeklyRange {
                weekday_mask: 0b111_1111,
                start: ClockTime::parse("09:00").unwrap(),
                end: ClockTime::parse("18:00").unwrap(),
            }],
            calendar: None,
        },
    });

    let mut urgent = rule(
        "urgent",
        1,
        Vec::new(),
        RoutingTarget::Policy {
            policy_id: id("pol-db"),
        },
    );
    urgent.priorities = vec![Priority::P1, Priority::P2];

    let table = table(vec![nights, urgent]);
    let route = |priority: Option<Priority>, hour: i64| {
        table
            .decide(&RoutingSubject {
                priority,
                at: TimestampMicros(hour * 3_600_000_000),
                labels: &BTreeMap::new(),
                affected_services: &[],
                severity: Severity::Warning,
                source_policy: &id(DEFAULT_POLICY_ID),
            })
            .escalation_policy_id
    };

    // 1970-01-01 was a Thursday: 20:00 is outside the day, 10:00 inside it.
    assert_eq!(route(None, 20), id("pol-night"));
    assert_eq!(route(Some(Priority::P1), 10), id("pol-db"));
    assert_eq!(route(Some(Priority::P3), 10), id(DEFAULT_POLICY_ID));
    assert_eq!(route(None, 10), id(DEFAULT_POLICY_ID));
}
