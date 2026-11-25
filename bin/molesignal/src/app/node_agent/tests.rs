// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::HashMap;

use async_trait::async_trait;
use domain::{
    alerting::rule::validate_conditions,
    intake::RawEvent,
    saved_view::{SavedView, SavedViewRepository},
    stream::{FieldDef, FieldIndexRule, FieldType, Schema, StreamIndexType, StreamSettings},
};
use kernel::{Error, Result, ids::Id, time::TimestampMicros};
use parking_lot::Mutex;
use serde_json::{Value, json};

use super::{
    EVENTS_STREAM,
    alerts::{ALERTS, TEMPLATE_LABEL, made_by},
    indexes::{missing, recommend},
    views::{VIEWS, install, installed},
};
use crate::app::alerting::realtime::{self, RealtimeAlerts};

#[derive(Default)]
struct Views(Mutex<HashMap<Id, SavedView>>);

#[async_trait]
impl SavedViewRepository for Views {
    async fn create(&self, view: SavedView) -> Result<SavedView> {
        let mut rows = self.0.lock();

        if rows.contains_key(&view.id) {
            return Err(Error::conflict("exists"));
        }

        rows.insert(view.id.clone(), view.clone());
        Ok(view)
    }

    async fn update(&self, view: SavedView) -> Result<SavedView> {
        self.0.lock().insert(view.id.clone(), view.clone());
        Ok(view)
    }

    async fn get_by_id(&self, id: &Id) -> Result<SavedView> {
        self.0
            .lock()
            .get(id)
            .cloned()
            .ok_or_else(|| Error::not_found("view"))
    }

    async fn get(&self, org: &Id, id: &Id) -> Result<SavedView> {
        let view = self.get_by_id(id).await?;

        if &view.org_id != org {
            return Err(Error::not_found("view"));
        }

        Ok(view)
    }

    async fn list(&self, org: &Id, _: bool) -> Result<Vec<SavedView>> {
        Ok(self
            .0
            .lock()
            .values()
            .filter(|view| &view.org_id == org)
            .cloned()
            .collect())
    }

    async fn delete(&self, _: &Id, id: &Id) -> Result<()> {
        self.0.lock().remove(id);
        Ok(())
    }
}

#[tokio::test]
async fn views_are_installed_once_per_organization_and_keep_a_persons_edits() {
    let repo = Views::default();
    let org = Id::from_string("one");
    let user = Id::from_string("member");

    assert!(installed(&repo, &org).await.unwrap().is_empty());

    let ids = install(&repo, &org, &user).await.unwrap();

    assert_eq!(ids.len(), VIEWS.len());
    assert_eq!(installed(&repo, &org).await.unwrap().len(), VIEWS.len());

    let mut edited = repo.get(&org, &ids[0]).await.unwrap();

    edited.statement = r#"SELECT * FROM "host-events""#.into();
    repo.update(edited.clone()).await.unwrap();
    repo.delete(&org, &ids[1]).await.unwrap();

    assert_eq!(install(&repo, &org, &user).await.unwrap(), ids);
    assert_eq!(
        repo.get(&org, &ids[0]).await.unwrap().statement,
        edited.statement
    );

    let other = Id::from_string("two");
    let other_ids = install(&repo, &other, &user).await.unwrap();

    assert!(other_ids.iter().all(|id| !ids.contains(id)));
}

/// A view lists the events of its kind with every field, newest first, from `host-events`: the
/// statement the logs page parses and adds a clicked value to.
#[test]
fn a_view_lists_the_events_of_its_kind_with_every_field() {
    for view in VIEWS {
        let statement = view.statement();
        let tables = query_language::extract_referenced_tables(&statement)
            .unwrap_or_else(|error| panic!("{}: {error}", view.key));

        assert_eq!(tables.len(), 1);
        assert_eq!(tables[0].name, EVENTS_STREAM);
        assert!(statement.starts_with(r#"SELECT * FROM "host-events""#));
        assert!(statement.contains("ORDER BY _timestamp DESC"));
    }

    let keys = VIEWS.map(|view| view.key);

    assert_eq!(keys, ["oom", "network", "hardware", "errors"]);

    for name in [
        "dropwatch",
        "tcp_retransmit",
        "netdev_events",
        "net_rx_latency",
    ] {
        assert!(VIEWS[1].condition.contains(name), "{name}");
    }

    for name in ["ras", "hungtask", "softlockup"] {
        assert!(VIEWS[2].condition.contains(name), "{name}");
    }
}

fn field(name: &str, index_type: StreamIndexType) -> FieldDef {
    FieldDef {
        name: name.into(),
        data_type: FieldType::Utf8,
        nullable: true,
        index_type,
        encrypted: false,
    }
}

#[test]
fn the_recommended_indexes_go_on_the_fields_that_have_none_and_no_rule() {
    let schema = Schema {
        fields: vec![
            field("host.id", StreamIndexType::None),
            field("container.id", StreamIndexType::Bloom),
            field("event.name", StreamIndexType::None),
            field("severity_text", StreamIndexType::None),
            field("body", StreamIndexType::None),
        ],
    };
    let settings = StreamSettings {
        index_rules: vec![FieldIndexRule {
            field: "severity_text".into(),
            enabled: false,
            index_type: StreamIndexType::None,
        }],
        ..StreamSettings::default()
    };

    // `event.domain` has not arrived, `container.id` has an index, `severity_text` a rule.
    assert_eq!(missing(&schema, &settings), ["host.id", "event.name"]);

    let now = TimestampMicros(42);
    let (schema, settings) = recommend(&schema, &settings, now).unwrap();
    let index = |name: &str| {
        schema
            .fields
            .iter()
            .find(|field| field.name == name)
            .unwrap()
            .index_type
    };

    assert_eq!(index("host.id"), StreamIndexType::Exact);
    assert_eq!(index("event.name"), StreamIndexType::Exact);
    assert_eq!(index("container.id"), StreamIndexType::Bloom);
    assert_eq!(index("severity_text"), StreamIndexType::None);
    assert_eq!(index("body"), StreamIndexType::None);
    assert_eq!(settings.index_rules.len(), 3);
    assert_eq!(settings.index_changed_at, Some(42));
    assert!(missing(&schema, &settings).is_empty());
    assert!(recommend(&schema, &settings, now).is_none());
}

fn event(fields: Value) -> RawEvent {
    let Value::Object(fields) = fields else {
        panic!("an event is an object");
    };

    RawEvent {
        timestamp: TimestampMicros(0),
        fields,
    }
}

/// Events as the agent writes them into `host-events`: its attributes, the resource, and the
/// fields of the event in `body`.
fn agent_event(name: &str, severity: &str, body: Value) -> RawEvent {
    event(json!({
        "host.name": "node-1",
        "host.id": "4f0c",
        "container.id": "c0ffee",
        "event.name": name,
        "event.domain": "host_kernel",
        "event.kind": "anomaly",
        "severity_text": severity,
        "body": body,
    }))
}

/// Which templates' rules count `event`, with every rule enabled.
fn counted_by(event: &RawEvent) -> Vec<&'static str> {
    let org = Id::from_string("org-1");
    let dashboard = Id::from_string("host-overview-org-1");

    ALERTS
        .iter()
        .filter(|alert| {
            let mut rule = alert.rule(&org, dashboard.clone());

            rule.enabled = true;

            let matcher = RealtimeAlerts::new();

            matcher.reload(vec![rule]);
            matcher
                .observe(
                    &org,
                    EVENTS_STREAM,
                    domain::stream::StreamType::LOGS,
                    std::slice::from_ref(event),
                )
                .is_some()
        })
        .map(|alert| alert.key)
        .collect()
}

#[test]
fn a_template_makes_a_disabled_real_time_rule_that_saves() {
    let org = Id::from_string("org-1");
    let dashboard = Id::from_string("host-overview-org-1");

    for alert in ALERTS {
        let mut rule = alert.rule(&org, dashboard.clone());

        assert!(!rule.enabled, "{}", alert.key);
        assert_eq!(rule.labels[TEMPLATE_LABEL], alert.key);
        rule.query.normalize(rule.kind).unwrap();
        realtime::validate(&rule).unwrap_or_else(|error| panic!("{}: {error}", alert.key));
        validate_conditions(rule.kind, &rule.trigger, &rule.thresholds).unwrap();
        rule.guide.normalize().unwrap();

        assert_eq!(rule.guide.dashboards, std::slice::from_ref(&dashboard));

        assert!(realtime::is_matched_at_intake(&rule), "{}", alert.key);
    }
}

#[test]
fn each_template_counts_only_the_events_it_is_about() {
    let cases = [
        (
            agent_event(
                "oom",
                "ERROR",
                json!({"victim": {"comm": "java"}, "trigger": {}}),
            ),
            vec!["oom_kill"],
        ),
        (
            agent_event("hungtask", "ERROR", json!({"pid": 7})),
            vec!["hungtask"],
        ),
        (
            agent_event("softlockup", "ERROR", json!({"cpu": 3})),
            vec!["softlockup"],
        ),
        (
            agent_event(
                "ras",
                "ERROR",
                json!({"dev": "mc0", "event": "EDAC", "type": "UncorrectedFatal"}),
            ),
            vec!["ras_uncorrected"],
        ),
        (
            agent_event(
                "ras",
                "WARN",
                json!({"dev": "mc0", "event": "EDAC", "type": "Corrected"}),
            ),
            vec![],
        ),
        (
            agent_event(
                "netdev_events",
                "INFO",
                json!({"ifname": "eth0", "linkstatus": "linkstatus_carrierdown"}),
            ),
            vec!["netdev_carrier_down"],
        ),
        (
            agent_event(
                "netdev_events",
                "INFO",
                json!({"ifname": "eth0", "linkstatus": "linkstatus_carrierup"}),
            ),
            vec![],
        ),
        (
            agent_event("netdev_txqueue_timeout", "ERROR", json!({"queue_index": 1})),
            vec!["netdev_txqueue_timeout"],
        ),
        (
            agent_event("dropwatch", "WARN", json!({"drop_reason": "x"})),
            vec![],
        ),
    ];

    for (event, expected) in cases {
        assert_eq!(counted_by(&event), expected, "{:?}", event.fields);
    }
}

#[test]
fn the_rule_of_a_template_is_found_by_its_label() {
    let org = Id::from_string("org-1");
    let dashboard = Id::from_string("host-overview-org-1");
    let rules = vec![
        ALERTS[0].rule(&org, dashboard.clone()),
        ALERTS[2].rule(&org, dashboard),
    ];

    assert!(made_by(&rules, &ALERTS[0]).is_some());
    assert!(made_by(&rules, &ALERTS[1]).is_none());
    assert_eq!(made_by(&rules, &ALERTS[2]).unwrap().id, rules[1].id);
}
