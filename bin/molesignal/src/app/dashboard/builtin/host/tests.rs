// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::{collections::HashSet, sync::Arc};

use arrow::array::{RecordBatch, StringArray, TimestampMicrosecondArray};
use datafusion::datasource::MemTable;
use serde_json::Value;

use super::*;
use crate::{
    app::{
        dashboard::{
            builtin::installed_version,
            contract_registry::builtin_dashboard_contract_resolver,
            validation::{DashboardValidationMode, ensure_dashboard_model_with},
        },
        node_agent::EVENTS_STREAM,
    },
    infra::query::{analyzer::session_context, arrow_json::batches_to_json},
};

/// The metrics Node Agent sends that the panels read: the names of its capability manifest
/// (`crates/core/host-observe/data/capability-manifest-v1.toml` in the agent's repository), and the
/// `/proc/vmstat` keys it reports for a host by default (`MS_VMSTAT_INCLUDED_ON_HOST`).
const AGENT_METRICS: &[&str] = &[
    "molesignal_host_cpu_util_total",
    "molesignal_host_cpu_util_usr",
    "molesignal_host_cpu_util_sys",
    "molesignal_host_loadavg_load1",
    "molesignal_host_loadavg_load5",
    "molesignal_host_loadavg_load15",
    "molesignal_host_loadavg_nr_uninterruptible",
    "molesignal_host_memory_vmstat_nr_free_pages",
    "molesignal_host_memory_vmstat_nr_active_anon",
    "molesignal_host_memory_vmstat_nr_inactive_anon",
    "molesignal_host_memory_vmstat_nr_active_file",
    "molesignal_host_memory_vmstat_nr_inactive_file",
    "molesignal_host_memory_vmstat_nr_slab_reclaimable",
    "molesignal_host_memory_vmstat_nr_slab_unreclaimable",
    "molesignal_host_diskio_read_bytes_total",
    "molesignal_host_diskio_written_bytes_total",
    "molesignal_host_netdev_receive_bytes_total",
    "molesignal_host_netdev_transmit_bytes_total",
    "molesignal_host_netdev_receive_errors_total",
    "molesignal_host_netdev_transmit_errors_total",
    "molesignal_host_netdev_receive_dropped_total",
    "molesignal_host_netdev_transmit_dropped_total",
];

fn model() -> Value {
    HostDashboard::Overview.model(&Id::from_string("dashboard-1"))
}

fn panels(model: &Value) -> Vec<&Value> {
    model["elements"].as_array().unwrap().iter().collect()
}

fn queries(panel: &Value) -> Vec<&Value> {
    panel["queries"].as_array().unwrap().iter().collect()
}

/// The statement of a SQL panel, with the variables a person has chosen.
fn bound(sql: &str, host: &str, interval: &str) -> String {
    sql.replace("${host:sqlstring}", &format!("'{host}'"))
        .replace("${interval}", interval)
}

/// The words of `expression` that name a metric of the agent.
fn metric_names(expression: &str) -> Vec<&str> {
    expression
        .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .filter(|word| word.starts_with("molesignal_host_"))
        .collect()
}

#[tokio::test]
async fn the_dashboard_meets_the_dashboard_contract() {
    let contracts = builtin_dashboard_contract_resolver()
        .active()
        .await
        .unwrap();

    ensure_dashboard_model_with(
        &model(),
        DashboardValidationMode::Native,
        &contracts.model_validator,
        &contracts.manifest,
    )
    .unwrap();
}

#[test]
fn the_dashboard_is_read_only_tagged_and_versioned() {
    let model = model();
    let dashboard = HostDashboard::Overview;

    assert_eq!(model["editable"], false);
    assert_eq!(model["uid"], dashboard.uid());
    assert_eq!(model["title"], dashboard.title());
    assert_eq!(
        model["tags"],
        serde_json::json!(["内置", "host", "node-agent"])
    );
    assert_eq!(installed_version(&model), VERSION);
    assert_eq!(
        dashboard.id(&Id::from_string("org-1")),
        Id::from_string("host-overview-org-1")
    );
}

#[test]
fn the_ids_of_panels_are_unique_and_the_panels_do_not_overlap() {
    let model = model();
    let panels = panels(&model);
    let ids = panels
        .iter()
        .map(|panel| panel["id"].as_str().unwrap())
        .collect::<HashSet<_>>();

    assert_eq!(ids.len(), panels.len());

    let boxes = panels
        .iter()
        .map(|panel| {
            let at = &panel["gridPos"];
            let get = |key: &str| at[key].as_u64().unwrap();

            (get("x"), get("y"), get("w"), get("h"), &panel["id"])
        })
        .collect::<Vec<_>>();

    for (index, left) in boxes.iter().enumerate() {
        assert!(left.0 + left.2 <= 24, "{} leaves the grid", left.4);

        for right in &boxes[index + 1..] {
            let apart = left.0 + left.2 <= right.0
                || right.0 + right.2 <= left.0
                || left.1 + left.3 <= right.1
                || right.1 + right.3 <= left.1;

            assert!(apart, "{} and {} overlap", left.4, right.4);
        }
    }
}

/// Every PromQL expression parses, reads metrics the agent sends from `host-metrics`, and keeps
/// to the chosen host's own series.
#[test]
fn a_metric_panel_reads_the_agents_metrics_of_the_chosen_host() {
    let model = model();
    let mut read = HashSet::new();

    for panel in panels(&model) {
        for query in queries(panel) {
            if query["dataSourceType"] != "metrics" {
                continue;
            }

            let expression = query["query"]["expression"].as_str().unwrap();

            promql_parser::parser::parse(expression)
                .unwrap_or_else(|error| panic!("{}: {error}: {expression}", panel["id"]));

            assert_eq!(query["query"]["streamName"], METRICS_STREAM);
            assert_eq!(query["format"], "time_series");
            assert!(
                expression.contains(r#""host.name"="$host""#)
                    && expression.contains(r#""container.id"="""#),
                "{}: {expression}",
                panel["id"]
            );

            for name in metric_names(expression) {
                assert!(
                    AGENT_METRICS.contains(&name),
                    "{} reads {name}, which the agent does not send",
                    panel["id"]
                );
                read.insert(name);
            }
        }
    }

    assert_eq!(
        read.len(),
        AGENT_METRICS.len(),
        "every metric listed is read"
    );

    for family in ["cpu_util", "loadavg", "memory", "diskio", "netdev"] {
        assert!(
            read.iter()
                .any(|name| name.starts_with(&format!("molesignal_host_{family}"))),
            "{family}"
        );
    }
}

#[test]
fn the_host_is_chosen_among_the_hosts_that_send_metrics() {
    let model = model();
    let variables = model["variables"].as_array().unwrap();
    let names = variables
        .iter()
        .map(|variable| variable["name"].as_str().unwrap())
        .collect::<Vec<_>>();

    assert_eq!(names, ["host", "interval"]);
    assert_eq!(
        variables[0]["query"]["expression"],
        "label_values(molesignal_host_loadavg_load1, host.name)"
    );
    assert_eq!(variables[0]["query"]["streamName"], METRICS_STREAM);
}

/// A table of `host-events` with the fields every event of the agent carries.
fn events() -> Arc<MemTable> {
    let batch = RecordBatch::try_from_iter([
        (
            "_timestamp",
            Arc::new(
                TimestampMicrosecondArray::from(vec![1_000_000, 61_000_000, 121_000_000])
                    .with_timezone("UTC"),
            ) as _,
        ),
        (
            "host.name",
            Arc::new(StringArray::from(vec!["node-1", "node-1", "node-2"])) as _,
        ),
        (
            "event.name",
            Arc::new(StringArray::from(vec!["oom", "dropwatch", "oom"])) as _,
        ),
        (
            "severity_text",
            Arc::new(StringArray::from(vec!["ERROR", "WARN", "ERROR"])) as _,
        ),
        (
            "event.domain",
            Arc::new(StringArray::from(vec!["host_kernel"; 3])) as _,
        ),
        (
            "event.kind",
            Arc::new(StringArray::from(vec!["anomaly"; 3])) as _,
        ),
    ])
    .unwrap();

    Arc::new(MemTable::try_new(batch.schema(), vec![vec![batch]]).unwrap())
}

#[tokio::test]
async fn an_event_panel_runs_on_the_fields_of_the_agents_events() {
    let ctx = session_context(0);

    ctx.register_table(EVENTS_STREAM, events()).unwrap();

    let model = model();
    let mut ran = 0;

    for panel in panels(&model) {
        for query in queries(panel) {
            if query["dataSourceType"] != "sql" {
                continue;
            }

            assert_eq!(query["query"]["streamName"], EVENTS_STREAM);

            let sql = bound(
                query["query"]["statement"].as_str().unwrap(),
                "node-1",
                "1 minute",
            );

            let batches = ctx
                .sql(&sql)
                .await
                .unwrap_or_else(|error| panic!("{}: {error}\n{sql}", panel["id"]))
                .collect()
                .await
                .unwrap_or_else(|error| panic!("{}: {error}\n{sql}", panel["id"]));

            let rows = batches_to_json(&batches).1;

            assert_eq!(rows.len(), 2, "{}: only node-1's events", panel["id"]);
            ran += 1;
        }
    }

    assert_eq!(ran, 3);
}

#[test]
fn the_content_of_a_version_does_not_change() {
    assert_eq!(model(), model());
    assert_eq!(model()["extensions"]["builtin"]["key"], "host-overview");
}
