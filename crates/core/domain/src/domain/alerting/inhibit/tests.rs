// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use super::*;
use crate::domain::alerting::{
    handling::Inhibition,
    incident::IncidentStatus,
    semantic_group::{LabelMatcher, MatchOp},
};

fn incident(id: &str, severity: Severity, labels: &[(&str, &str)]) -> Incident {
    Incident {
        id: Id::from_string(id),
        org_id: Id::from_string("org-1"),
        rule_id: Id::from_string(format!("rule-{id}")),
        escalation_policy_id: Id::from_string("default"),
        status: IncidentStatus::Open,
        severity,
        summary: format!("incident {id}"),
        fingerprint: format!("fp-{id}"),
        current_step: 0,
        current_loop: 0,
        current_step_started_at: TimestampMicros(1_000),
        assignees: Vec::new(),
        labels: labels
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect(),
        annotations: BTreeMap::new(),
        trace_ids: Vec::new(),
        host_ids: Vec::new(),
        affected_services: Vec::new(),
        triggering_query: None,
        created_at: TimestampMicros(1_000),
        acknowledged_at: None,
        acknowledged_by: None,
        resolved_at: None,
        resolved_by: None,
        team_id: None,
        routing: None,
        handling: Default::default(),
        revision: 0,
    }
}

fn eq(label: &str, value: &str) -> LabelMatcher {
    LabelMatcher {
        label: label.into(),
        op: MatchOp::Eq,
        value: value.into(),
    }
}

/// 同一个 `service` 上，更严重的压住次要的。
fn same_service() -> InhibitRule {
    InhibitRule {
        id: Id::from_string("rule"),
        org_id: Id::from_string("org-1"),
        name: "same service".into(),
        enabled: true,
        source: InhibitSource::default(),
        target: InhibitTarget::default(),
        equal: vec!["service".into()],
        more_severe_only: true,
        wait_secs: DEFAULT_WAIT_SECS,
        linger_secs: DEFAULT_LINGER_SECS,
        comment: String::new(),
        created_by: None,
        created_at: TimestampMicros(1),
        updated_at: TimestampMicros(1),
    }
}

fn nobody_silenced(_: &Incident) -> bool {
    false
}

#[test]
fn a_worse_incident_on_the_same_service_suppresses_the_lesser_one() {
    let incidents = [
        incident("db", Severity::Critical, &[("service", "checkout")]),
        incident("slow", Severity::Warning, &[("service", "checkout")]),
        incident("other", Severity::Warning, &[("service", "search")]),
    ];

    let found = resolve(&[same_service()], &incidents, nobody_silenced);

    assert_eq!(found.len(), 1);
    let inhibited = &found[&Id::from_string("slow")];
    assert_eq!(inhibited.source_incident_id, Id::from_string("db"));
    assert_eq!(inhibited.rule_name, "same service");
}

#[test]
fn incidents_of_the_same_severity_do_not_suppress_each_other_by_default() {
    let incidents = [
        incident("a", Severity::Critical, &[("service", "checkout")]),
        incident("b", Severity::Critical, &[("service", "checkout")]),
    ];

    assert!(resolve(&[same_service()], &incidents, nobody_silenced).is_empty());
}

#[test]
fn a_shared_label_must_be_there_on_both_sides() {
    let incidents = [
        incident("worse", Severity::Critical, &[]),
        incident("lesser", Severity::Warning, &[]),
    ];

    assert!(resolve(&[same_service()], &incidents, nobody_silenced).is_empty());
}

#[test]
fn the_service_of_an_incident_counts_as_its_label() {
    let mut worse = incident("worse", Severity::Critical, &[]);
    worse.affected_services = vec!["checkout".into()];

    let incidents = [
        worse,
        incident("lesser", Severity::Warning, &[("service", "checkout")]),
    ];

    let found = resolve(&[same_service()], &incidents, nobody_silenced);
    assert!(found.contains_key(&Id::from_string("lesser")));
}

#[test]
fn a_dependency_rule_suppresses_whatever_the_severities() {
    let mut rule = same_service();
    rule.equal = Vec::new();
    rule.more_severe_only = false;
    rule.source.matchers = vec![eq("service", "database")];
    rule.target.matchers = vec![eq("tier", "api")];

    let incidents = [
        incident("db", Severity::Warning, &[("service", "database")]),
        incident(
            "api",
            Severity::Critical,
            &[("service", "checkout"), ("tier", "api")],
        ),
        incident("web", Severity::Critical, &[("tier", "web")]),
    ];

    let found = resolve(&[rule], &incidents, nobody_silenced);
    assert_eq!(found.len(), 1);
    assert!(found.contains_key(&Id::from_string("api")));
}

#[test]
fn severity_bounds_narrow_both_sides() {
    let mut rule = same_service();
    rule.source.min_severity = Some(Severity::Critical);
    rule.target.max_severity = Some(Severity::Warning);

    let incidents = [
        incident("error", Severity::Error, &[("service", "x")]),
        incident("warning", Severity::Warning, &[("service", "x")]),
    ];
    assert!(resolve(std::slice::from_ref(&rule), &incidents, nobody_silenced).is_empty());

    let incidents = [
        incident("critical", Severity::Critical, &[("service", "x")]),
        incident("error", Severity::Error, &[("service", "x")]),
        incident("warning", Severity::Warning, &[("service", "x")]),
    ];

    let found = resolve(&[rule], &incidents, nobody_silenced);
    assert_eq!(found.len(), 1);
    assert!(found.contains_key(&Id::from_string("warning")));
}

#[test]
fn a_silenced_incident_suppresses_nothing() {
    let incidents = [
        incident("db", Severity::Critical, &[("service", "x")]),
        incident("slow", Severity::Warning, &[("service", "x")]),
    ];

    let found = resolve(&[same_service()], &incidents, |incident| {
        incident.id.as_str() == "db"
    });

    assert!(found.is_empty());
}

#[test]
fn an_acknowledged_incident_keeps_suppressing_and_a_resolved_one_stops() {
    let mut source = incident("db", Severity::Critical, &[("service", "x")]);
    let target = incident("slow", Severity::Warning, &[("service", "x")]);

    source.status = IncidentStatus::Acknowledged;
    assert_eq!(
        resolve(
            &[same_service()],
            &[source.clone(), target.clone()],
            nobody_silenced
        )
        .len(),
        1
    );

    source.status = IncidentStatus::Resolved;
    assert!(resolve(&[same_service()], &[source, target], nobody_silenced).is_empty());
}

#[test]
fn merged_incidents_count_on_neither_side() {
    let mut source = incident("db", Severity::Critical, &[("service", "x")]);
    source.handling.merged_into = Some(Id::from_string("main"));
    let target = incident("slow", Severity::Warning, &[("service", "x")]);

    assert!(resolve(&[same_service()], &[source, target], nobody_silenced).is_empty());
}

#[test]
fn a_person_decides_about_what_a_person_opened_or_lifted() {
    let source = incident("db", Severity::Critical, &[("service", "x")]);
    let manual = incident(
        "manual",
        Severity::Warning,
        &[("service", "x"), (SOURCE_LABEL, MANUAL_SOURCE)],
    );
    let mut lifted = incident("lifted", Severity::Warning, &[("service", "x")]);
    lifted.handling.inhibition_lifted_by = Some(Id::from_string("u1"));

    assert!(
        resolve(
            &[same_service()],
            &[source, manual, lifted],
            nobody_silenced
        )
        .is_empty()
    );
}

#[test]
fn a_person_opened_incident_may_suppress_others() {
    let manual = incident(
        "declared",
        Severity::Critical,
        &[("service", "x"), (SOURCE_LABEL, MANUAL_SOURCE)],
    );
    let target = incident("slow", Severity::Warning, &[("service", "x")]);

    let found = resolve(&[same_service()], &[manual, target], nobody_silenced);
    assert!(found.contains_key(&Id::from_string("slow")));
}

#[test]
fn suppression_chains_down_from_the_incident_that_notifies() {
    let mut network_to_db = same_service();
    network_to_db.equal = Vec::new();
    network_to_db.more_severe_only = false;
    network_to_db.source.matchers = vec![eq("service", "network")];
    network_to_db.target.matchers = vec![eq("service", "db")];

    let mut db_to_api = network_to_db.clone();
    db_to_api.id = Id::from_string("rule-2");
    db_to_api.source.matchers = vec![eq("service", "db")];
    db_to_api.target.matchers = vec![eq("service", "api")];

    let incidents = [
        incident("network", Severity::Critical, &[("service", "network")]),
        incident("db", Severity::Critical, &[("service", "db")]),
        incident("api", Severity::Critical, &[("service", "api")]),
    ];

    let found = resolve(&[network_to_db, db_to_api], &incidents, nobody_silenced);

    assert_eq!(found.len(), 2);
    assert_eq!(
        found[&Id::from_string("db")].source_incident_id,
        Id::from_string("network")
    );
    assert_eq!(
        found[&Id::from_string("api")].source_incident_id,
        Id::from_string("db")
    );
}

#[test]
fn incidents_suppressing_each_other_leave_the_worst_to_notify() {
    let mut a_to_b = same_service();
    a_to_b.equal = Vec::new();
    a_to_b.more_severe_only = false;
    a_to_b.source.matchers = vec![eq("service", "a")];
    a_to_b.target.matchers = vec![eq("service", "b")];

    let mut b_to_a = a_to_b.clone();
    b_to_a.id = Id::from_string("rule-2");
    b_to_a.source.matchers = vec![eq("service", "b")];
    b_to_a.target.matchers = vec![eq("service", "a")];

    let incidents = [
        incident("a", Severity::Warning, &[("service", "a")]),
        incident("b", Severity::Critical, &[("service", "b")]),
    ];

    let found = resolve(
        &[a_to_b.clone(), b_to_a.clone()],
        &incidents,
        nobody_silenced,
    );

    assert_eq!(found.len(), 1);
    assert!(found.contains_key(&Id::from_string("a")));

    // 同样严重时先开的留下；再算一遍结果不变。
    let mut first = incident("a", Severity::Warning, &[("service", "a")]);
    first.created_at = TimestampMicros(500);
    let later = incident("b", Severity::Warning, &[("service", "b")]);
    let same = [first, later];

    for _ in 0..3 {
        let found = resolve(&[a_to_b.clone(), b_to_a.clone()], &same, nobody_silenced);
        assert_eq!(found.len(), 1);
        assert!(found.contains_key(&Id::from_string("b")));
    }
}

#[test]
fn a_rule_that_is_off_does_nothing() {
    let mut rule = same_service();
    rule.enabled = false;

    let incidents = [
        incident("db", Severity::Critical, &[("service", "x")]),
        incident("slow", Severity::Warning, &[("service", "x")]),
    ];

    assert!(resolve(&[rule], &incidents, nobody_silenced).is_empty());
}

#[test]
fn a_rule_must_say_who_suppresses() {
    let mut rule = same_service();
    rule.name = "  named  ".into();
    rule.equal = vec![" service ".into(), "service".into(), " ".into()];
    rule.validate().unwrap();
    assert_eq!(rule.name, "named");
    assert_eq!(rule.equal, vec!["service".to_string()]);

    let mut everything = same_service();
    everything.equal = Vec::new();
    assert!(everything.validate().is_err());

    let mut unnamed = same_service();
    unnamed.name = " ".into();
    assert!(unnamed.validate().is_err());

    let mut bad_regex = same_service();
    bad_regex.target.matchers = vec![LabelMatcher {
        label: "service".into(),
        op: MatchOp::Re,
        value: "(".into(),
    }];
    assert!(bad_regex.validate().is_err());

    let mut reserved = same_service();
    reserved.source.matchers = vec![eq(INCIDENT_ID_MATCHER_LABEL, "x")];
    assert!(reserved.validate().is_err());

    let mut long_wait = same_service();
    long_wait.wait_secs = MAX_WAIT_SECS + 1;
    assert!(long_wait.validate().is_err());

    let mut long_linger = same_service();
    long_linger.linger_secs = MAX_LINGER_SECS + 1;
    assert!(long_linger.validate().is_err());
}

#[test]
fn a_new_incident_waits_only_when_it_could_be_suppressed() {
    let rule = same_service();
    let lesser = incident("slow", Severity::Warning, &[("service", "x")]);

    assert_eq!(
        wait_until(std::slice::from_ref(&rule), &lesser),
        Some(TimestampMicros(1_000 + 60_000_000))
    );

    // 缺这个标签就不会被压。
    let unlabelled = incident("slow", Severity::Warning, &[]);
    assert_eq!(wait_until(std::slice::from_ref(&rule), &unlabelled), None);

    // 最严重的不会有更严重的来压它。
    let worst = incident("db", Severity::Critical, &[("service", "x")]);
    assert_eq!(wait_until(std::slice::from_ref(&rule), &worst), None);

    // 人开的不等。
    let manual = incident(
        "m",
        Severity::Warning,
        &[("service", "x"), (SOURCE_LABEL, MANUAL_SOURCE)],
    );
    assert_eq!(wait_until(std::slice::from_ref(&rule), &manual), None);

    // 规则说不等，也要等到派发判定一次。
    let mut no_wait = rule.clone();
    no_wait.wait_secs = 0;
    assert_eq!(
        wait_until(&[no_wait], &lesser),
        Some(TimestampMicros(1_000 + i64::from(SETTLE_SECS) * 1_000_000))
    );

    let mut off = rule;
    off.enabled = false;
    assert_eq!(wait_until(&[off], &lesser), None);
}

fn matched(source: &str) -> Inhibited {
    Inhibited {
        rule_id: Id::from_string("rule"),
        rule_name: "same service".into(),
        source_incident_id: Id::from_string(source),
        source_summary: format!("incident {source}"),
    }
}

#[test]
fn being_suppressed_is_recorded_once_and_follows_the_source() {
    let rules = [same_service()];
    let mut target = incident("slow", Severity::Warning, &[("service", "x")]);
    target.handling.inhibition_wait_until = Some(TimestampMicros(9_000_000));

    let change = settle(
        &mut target,
        Some(&matched("db")),
        &rules,
        |_| true,
        TimestampMicros(2_000_000),
    );
    assert_eq!(change, Some(InhibitionChange::Started));

    let state = target.handling.inhibition.clone().unwrap();
    assert_eq!(state.since, TimestampMicros(2_000_000));
    assert_eq!(state.source_incident_id, Id::from_string("db"));
    assert_eq!(target.handling.inhibition_wait_until, None);
    assert!(target.handling.inhibition_holds(TimestampMicros(3_000_000)));

    // 同一个源事件：不用写。
    assert_eq!(
        settle(
            &mut target,
            Some(&matched("db")),
            &rules,
            |_| true,
            TimestampMicros(3_000_000),
        ),
        None
    );

    // 换了一个源事件：起始时间不变。
    assert_eq!(
        settle(
            &mut target,
            Some(&matched("network")),
            &rules,
            |_| true,
            TimestampMicros(4_000_000),
        ),
        Some(InhibitionChange::Updated)
    );

    let state = target.handling.inhibition.clone().unwrap();
    assert_eq!(state.since, TimestampMicros(2_000_000));
    assert_eq!(state.source_incident_id, Id::from_string("network"));
}

#[test]
fn a_recovered_source_keeps_suppressing_for_a_while_then_lets_go() {
    let rules = [same_service()];
    let mut target = incident("slow", Severity::Warning, &[("service", "x")]);

    settle(
        &mut target,
        Some(&matched("db")),
        &rules,
        |_| true,
        TimestampMicros(1_000_000),
    );

    // 源事件恢复：倒计时 120 秒，通知还压着。
    let recovered_at = TimestampMicros(10_000_000);
    assert_eq!(
        settle(&mut target, None, &rules, |_| false, recovered_at),
        Some(InhibitionChange::Updated)
    );

    let ends_at = TimestampMicros(130_000_000);
    assert_eq!(
        target.handling.inhibition.as_ref().unwrap().ending_at,
        Some(ends_at)
    );
    assert!(target.handling.inhibition_holds(recovered_at));

    // 没到点：不用写。
    assert_eq!(
        settle(
            &mut target,
            None,
            &rules,
            |_| false,
            TimestampMicros(129_000_000)
        ),
        None
    );

    // 倒计时里又来了源事件：接着压。
    let mut again = target.clone();
    assert_eq!(
        settle(
            &mut again,
            Some(&matched("db2")),
            &rules,
            |_| true,
            TimestampMicros(100_000_000)
        ),
        Some(InhibitionChange::Updated)
    );
    assert_eq!(again.handling.inhibition.as_ref().unwrap().ending_at, None);

    // 到点：放开，这一步从现在算。
    assert_eq!(
        settle(&mut target, None, &rules, |_| false, ends_at),
        Some(InhibitionChange::Ended {
            reason: "source_recovered"
        })
    );
    assert_eq!(target.handling.inhibition, None);
    assert_eq!(target.current_step_started_at, ends_at);
    assert!(!target.handling.inhibition_holds(ends_at));
}

#[test]
fn a_removed_rule_lets_go_at_once() {
    let mut target = incident("slow", Severity::Warning, &[("service", "x")]);

    settle(
        &mut target,
        Some(&matched("db")),
        &[same_service()],
        |_| true,
        TimestampMicros(1_000_000),
    );

    // 源事件还在，规则删了。
    assert_eq!(
        settle(&mut target, None, &[], |_| true, TimestampMicros(2_000_000)),
        Some(InhibitionChange::Ended {
            reason: "rule_changed"
        })
    );
}

#[test]
fn the_step_starts_when_the_wait_is_over() {
    let mut target = incident("slow", Severity::Warning, &[("service", "x")]);
    target.handling.inhibition_wait_until = Some(TimestampMicros(5_000_000));

    assert!(target.handling.inhibition_holds(TimestampMicros(4_000_000)));
    assert_eq!(
        settle(&mut target, None, &[], |_| true, TimestampMicros(4_000_000)),
        None
    );

    assert_eq!(
        settle(&mut target, None, &[], |_| true, TimestampMicros(5_000_000)),
        Some(InhibitionChange::WaitOver)
    );
    assert_eq!(target.handling.inhibition_wait_until, None);
    assert_eq!(target.current_step_started_at, TimestampMicros(5_000_000));
}

#[test]
fn lifting_clears_the_state_and_keeps_the_incident_out() {
    let mut target = incident("slow", Severity::Warning, &[("service", "x")]);

    settle(
        &mut target,
        Some(&matched("db")),
        &[same_service()],
        |_| true,
        TimestampMicros(1_000_000),
    );

    let who = Id::from_string("u1");
    assert!(target.handling.lift_inhibition(&who));
    assert!(!target.handling.inhibition_holds(TimestampMicros(2_000_000)));
    assert_eq!(target.handling.inhibition_lifted_by, Some(who.clone()));
    assert!(!target.handling.lift_inhibition(&who));

    let source = incident("db", Severity::Critical, &[("service", "x")]);
    assert!(resolve(&[same_service()], &[source, target], nobody_silenced).is_empty());
}

#[test]
fn the_preview_lists_who_suppresses_whom_under_one_rule_even_if_it_is_off() {
    let mut rule = same_service();
    rule.enabled = false;

    let incidents = [
        incident("db", Severity::Critical, &[("service", "x")]),
        incident("slow", Severity::Warning, &[("service", "x")]),
        incident("other", Severity::Warning, &[("service", "y")]),
    ];

    let pairs = preview(&rule, &incidents, nobody_silenced);

    assert_eq!(pairs.len(), 1);
    assert_eq!(pairs[0].source_summary, "incident db");
    assert_eq!(pairs[0].target_summary, "incident slow");
    assert_eq!(pairs[0].target_severity, Severity::Warning);
}

#[test]
fn counts_are_per_rule_and_only_of_active_incidents() {
    let mut held = incident("slow", Severity::Warning, &[]);
    held.handling.inhibition = Some(Inhibition {
        rule_id: Id::from_string("rule"),
        rule_name: "r".into(),
        source_incident_id: Id::from_string("db"),
        source_summary: "db".into(),
        since: TimestampMicros(1),
        ending_at: None,
    });
    let mut over = held.clone();
    over.status = IncidentStatus::Resolved;

    let counts = inhibiting_counts(&[held, over]);
    assert_eq!(counts[&Id::from_string("rule")], 1);
}

#[test]
fn a_rule_saved_without_the_buffers_takes_the_defaults() {
    let rule: InhibitRule = serde_json::from_value(serde_json::json!({
        "id": "r", "orgId": "o", "name": "n", "enabled": true,
        "source": {}, "target": {}, "equal": ["service"],
        "createdAt": 1, "updatedAt": 1,
    }))
    .unwrap();

    assert!(rule.more_severe_only);
    assert_eq!(rule.wait_secs, DEFAULT_WAIT_SECS);
    assert_eq!(rule.linger_secs, DEFAULT_LINGER_SECS);
}
