// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::BTreeMap;

use domain::apm::{
    InstanceObservation, ServiceChangeCount, ServiceIdentity, ServiceObservation,
    VersionObservation,
};
use kernel::{
    ids::Id,
    time::{TimeRange, TimestampMicros},
};

use super::{
    Grade, LookState, NamedIn,
    grade::{Inputs, assemble},
    looks::{LogLook, MetricLook, NameLines, plausible_trace_id},
};
use crate::app::apm::ServiceInventory;

const HOUR: i64 = 3_600_000_000;

fn identity(name: &str, environment: Option<&str>) -> ServiceIdentity {
    ServiceIdentity::new(Some("shop"), Some(name), environment, None)
}

fn observation(name: &str, environment: Option<&str>) -> ServiceObservation {
    ServiceObservation {
        org_id: Id("org".into()),
        service: identity(name, environment),
        first_seen_at: TimestampMicros(0),
        last_seen_at: TimestampMicros(HOUR),
        runtime_language: None,
        telemetry_sdk_name: None,
        telemetry_sdk_version: None,
        recent_instance_count: 0,
        entry_kinds: Vec::new(),
    }
}

fn version(name: &str) -> VersionObservation {
    VersionObservation {
        org_id: Id("org".into()),
        service: identity(name, Some("prod")),
        version: "1.2.0".into(),
        first_seen_at: TimestampMicros(0),
        last_seen_at: TimestampMicros(HOUR),
        observation_count: 3,
    }
}

fn instance(name: &str, attributes: &[(&str, &str)]) -> InstanceObservation {
    InstanceObservation {
        org_id: Id("org".into()),
        service: identity(name, Some("prod")),
        instance_id: format!("{name}-1"),
        version: Some("1.2.0".into()),
        first_seen_at: TimestampMicros(0),
        last_seen_at: TimestampMicros(HOUR),
        attributes: attributes
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect::<BTreeMap<_, _>>(),
    }
}

fn lines(lines: u64, traced: u64, sampled: u64, matched: u64) -> NameLines {
    NameLines {
        streams: ["default".to_owned()].into(),
        lines,
        traced,
        sampled,
        matched,
    }
}

fn inputs(inventory: ServiceInventory, logs: LogLook, metrics: MetricLook) -> Inputs {
    Inputs {
        window: TimeRange::new(TimestampMicros(0), TimestampMicros(HOUR)),
        changes_since: TimestampMicros(-7 * 24 * HOUR),
        inventory,
        changes: Some(vec![ServiceChangeCount {
            service: "checkout".into(),
            count: 2,
            last_at: TimestampMicros(HOUR / 2),
        }]),
        logs,
        metrics,
    }
}

#[test]
fn a_well_tied_service_is_good_every_way() {
    let inventory = ServiceInventory {
        services: vec![observation("checkout", Some("prod"))],
        versions: vec![version("checkout")],
        instances: vec![instance("checkout", &[("k8s.pod.name", "checkout-7d9")])],
    };

    let mut logs = LogLook::default();
    logs.by_name
        .insert("checkout".into(), lines(100, 95, 10, 9));

    let mut metrics = MetricLook::default();
    metrics
        .by_name
        .insert("checkout".into(), ["metrics".to_owned()].into());

    let report = assemble(inputs(inventory, logs, metrics));
    let service = &report.services[0];

    assert_eq!(service.grades.trace_ids, Grade::Good);
    assert_eq!(service.grades.resource, Grade::Good);
    assert_eq!(service.grades.metrics, Grade::Good);
    assert_eq!(service.grades.changes, Grade::Good);
    assert_eq!(service.changes.count, 2);
    assert!(report.unmatched.is_empty());
}

#[test]
fn trace_ids_are_partial_when_few_lines_carry_them_or_few_are_kept() {
    let inventory = ServiceInventory {
        services: vec![
            observation("few", Some("prod")),
            observation("lost", Some("prod")),
            observation("none", Some("prod")),
        ],
        ..ServiceInventory::default()
    };

    let mut logs = LogLook::default();
    logs.by_name.insert("few".into(), lines(100, 40, 0, 0));
    logs.by_name.insert("lost".into(), lines(100, 100, 10, 2));
    logs.by_name.insert("none".into(), lines(100, 0, 0, 0));

    let report = assemble(inputs(inventory, logs, MetricLook::default()));

    let grade = |name: &str| {
        report
            .services
            .iter()
            .find(|service| service.service.name == name)
            .map(|service| service.grades.trace_ids)
    };

    assert_eq!(grade("few"), Some(Grade::Partial));
    assert_eq!(grade("lost"), Some(Grade::Partial));
    assert_eq!(grade("none"), Some(Grade::Missing));
}

#[test]
fn a_failed_look_reads_as_unknown_not_missing() {
    let inventory = ServiceInventory {
        services: vec![observation("checkout", Some("prod"))],
        ..ServiceInventory::default()
    };

    let logs = LogLook {
        failed: true,
        ..LogLook::default()
    };

    let metrics = MetricLook {
        failed: true,
        ..MetricLook::default()
    };

    let mut inputs = inputs(inventory, logs, metrics);
    inputs.changes = None;

    let service = &assemble(inputs).services[0];

    assert_eq!(service.logs.state, LookState::Unknown);
    assert_eq!(service.grades.trace_ids, Grade::Unknown);
    assert_eq!(service.grades.metrics, Grade::Unknown);
    assert_eq!(service.grades.changes, Grade::Unknown);
}

#[test]
fn resource_is_graded_by_what_the_spans_say() {
    let inventory = ServiceInventory {
        services: vec![
            observation("cart", None),
            observation("search", Some("prod")),
        ],
        versions: vec![version("search")],
        instances: vec![
            instance("cart", &[("host.name", "web-1")]),
            instance("search", &[("process.runtime.name", "go")]),
        ],
    };

    let report = assemble(inputs(inventory, LogLook::default(), MetricLook::default()));
    let cart = &report.services[0];
    let search = &report.services[1];

    // The cart's instance was recorded under prod, the service under no environment.
    assert!(!cart.resource.environment && !cart.resource.version && !cart.resource.placement);
    assert_eq!(cart.grades.resource, Grade::Missing);
    assert_eq!(cart.grades.changes, Grade::Missing);
    assert!(search.resource.version && search.resource.instance && !search.resource.placement);
    assert_eq!(search.grades.resource, Grade::Partial);
}

#[test]
fn names_no_service_answers_to_are_listed_busiest_first() {
    let inventory = ServiceInventory {
        services: vec![observation("checkout", Some("prod"))],
        ..ServiceInventory::default()
    };

    let mut logs = LogLook::default();
    logs.by_name.insert("checkout".into(), lines(10, 10, 0, 0));
    logs.by_name
        .insert("checkout-svc".into(), lines(500, 0, 0, 0));
    logs.by_name.insert("cron".into(), lines(20, 0, 0, 0));

    let mut metrics = MetricLook::default();
    metrics
        .by_name
        .insert("node-exporter".into(), ["metrics".to_owned()].into());

    let report = assemble(inputs(inventory, logs, metrics));

    let names = report
        .unmatched
        .iter()
        .map(|name| (name.name.as_str(), name.signal))
        .collect::<Vec<_>>();

    assert_eq!(
        names,
        vec![
            ("checkout-svc", NamedIn::Logs),
            ("cron", NamedIn::Logs),
            ("node-exporter", NamedIn::Metrics),
        ]
    );
}

#[test]
fn only_hex_trace_ids_are_looked_for() {
    assert!(plausible_trace_id("4bf92f3577b34da6a3ce929d0e0e4736"));
    assert!(plausible_trace_id("4bf92f35-77b3-4da6-a3ce-929d0e0e4736"));
    assert!(!plausible_trace_id("abc"));
    assert!(!plausible_trace_id("4bf92f3577b34da6' OR '1'='1"));
}
