// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde_json::{Value, json};

use super::*;
use crate::domain::dashboard::authoring::{
    AuthoringElement, AuthoringQuery, AuthoringRefresh, AuthoringSize, AuthoringTimeRange,
    AuthoringVariable, PanelAuthoringSpec, TextAuthoringSpec, TextMode, VisualizationIntent,
};

const VALID_AUTHORING: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/contracts/dashboard/fixtures/valid/authoring-v1-promql.json"
));

#[test]
fn compilation_is_deterministic_and_contract_valid() {
    let spec: DashboardAuthoringSpec = serde_json::from_str(VALID_AUTHORING).unwrap();
    let first = DashboardAuthoringCompiler::compile(&spec).unwrap();
    let second = DashboardAuthoringCompiler::compile(&spec).unwrap();
    let round_trip: DashboardAuthoringSpec =
        serde_json::from_value(serde_json::to_value(&spec).unwrap()).unwrap();
    let third = DashboardAuthoringCompiler::compile(&round_trip).unwrap();
    assert_eq!(first.model, second.model);
    assert_eq!(first.model_hash, second.model_hash);
    assert_eq!(first.model_hash, third.model_hash);
    assert_eq!(first.model_hash.len(), 64);
    assert!(dashboard_model_validator().is_valid(&first.model));
}

#[test]
fn every_catalog_visualization_query_combination_compiles_with_golden_defaults() {
    let manifest = visualization_manifest();
    for capability in &manifest.visualizations {
        if capability.visualization_type == "text" {
            let compiled = DashboardAuthoringCompiler::compile(&text_spec()).unwrap();
            assert_eq!(compiled.model["elements"][0]["kind"], "text");
            assert_eq!(compiled.model["elements"][0]["mode"], "markdown");
            continue;
        }
        for query_kind in &capability.compatible_query_kinds {
            let spec = spec_with_panels(vec![panel(
                &capability.visualization_type,
                None,
                query(query_kind, &capability.visualization_type),
            )]);
            let compiled = DashboardAuthoringCompiler::compile(&spec).unwrap_or_else(|error| {
                panic!(
                    "{} + {} did not compile: {error}",
                    capability.visualization_type, query_kind
                )
            });
            let panel = &compiled.model["elements"][0];
            assert_eq!(
                panel["visualization"]["type"],
                capability.visualization_type
            );
            assert_eq!(
                panel["visualization"]["schemaVersion"],
                capability.option_schema_version
            );
            assert_eq!(
                panel["visualization"]["options"],
                Value::Object(capability.default_options.clone())
            );
            assert_eq!(panel["queries"][0]["refId"], "A");
            assert!(dashboard_model_validator().is_valid(&compiled.model));
        }
    }
}

#[test]
fn layout_ids_variables_and_defaults_are_stable() {
    let mut spec = spec_with_panels(vec![
        panel("stat", Some(AuthoringSize::Small), query("promql", "stat")),
        panel(
            "time_series",
            Some(AuthoringSize::Medium),
            query("promql", "time_series"),
        ),
        panel(
            "bar_chart",
            Some(AuthoringSize::Wide),
            query("sql", "bar_chart"),
        ),
        panel("table", Some(AuthoringSize::Full), query("trace", "table")),
    ]);
    spec.variables = vec![AuthoringVariable::Custom {
        name: "service".into(),
        label: Some("Service".into()),
        values: vec![json!("api"), json!("worker")],
        default_value: Some(json!("api")),
        multi: false,
        include_all: false,
    }];
    let compiled = DashboardAuthoringCompiler::compile(&spec).unwrap();
    let elements = compiled.model["elements"].as_array().unwrap();
    assert_eq!(elements[0]["id"], "panel-001");
    assert_eq!(elements[1]["id"], "panel-002");
    assert_eq!(elements[2]["id"], "panel-003");
    assert_eq!(elements[3]["id"], "panel-004");
    assert_eq!(
        elements[0]["gridPos"],
        json!({"x": 0, "y": 0, "w": 6, "h": 8})
    );
    assert_eq!(
        elements[1]["gridPos"],
        json!({"x": 6, "y": 0, "w": 12, "h": 10})
    );
    assert_eq!(
        elements[2]["gridPos"],
        json!({"x": 0, "y": 10, "w": 18, "h": 10})
    );
    assert_eq!(
        elements[3]["gridPos"],
        json!({"x": 0, "y": 20, "w": 24, "h": 12})
    );
    assert_eq!(compiled.model["variables"][0]["id"], "variable-001");
    assert_eq!(compiled.model["refreshSettings"]["defaultInterval"], "30s");
    assert_eq!(compiled.model["timeSettings"]["defaultFrom"], "now-1h");
}

#[test]
fn rejects_unsupported_contract_and_semantic_combinations() {
    let mut unsupported = spec_with_panels(vec![panel(
        "time_series",
        None,
        query("promql", "time_series"),
    )]);
    unsupported.authoring_version = 2;
    let error = DashboardAuthoringCompiler::compile(&unsupported).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("unsupported Dashboard authoring")
    );

    let incompatible = spec_with_panels(vec![panel("logs", None, query("promql", "logs"))]);
    let error = DashboardAuthoringCompiler::compile(&incompatible).unwrap_err();
    assert!(error.to_string().contains("validation"));
}

fn spec_with_panels(panels: Vec<PanelAuthoringSpec>) -> DashboardAuthoringSpec {
    DashboardAuthoringSpec {
        authoring_version: 1,
        title: "Compiler golden".into(),
        description: None,
        tags: vec!["ai".into()],
        folder_id: None,
        time_range: Some(AuthoringTimeRange {
            from: "now-1h".into(),
            to: "now".into(),
            timezone: Some("browser".into()),
        }),
        refresh: Some(AuthoringRefresh::Interval {
            interval: "30s".into(),
        }),
        variables: Vec::new(),
        elements: panels.into_iter().map(AuthoringElement::Panel).collect(),
    }
}

fn text_spec() -> DashboardAuthoringSpec {
    let mut spec = spec_with_panels(Vec::new());
    spec.elements
        .push(AuthoringElement::Text(TextAuthoringSpec {
            title: Some("Context".into()),
            content: "Generated from reviewed data.".into(),
            mode: Some(TextMode::Markdown),
            size: Some(AuthoringSize::Full),
            transparent: Some(false),
        }));
    spec
}

fn panel(
    visualization_type: &str,
    size: Option<AuthoringSize>,
    query: AuthoringQuery,
) -> PanelAuthoringSpec {
    PanelAuthoringSpec {
        title: format!("{visualization_type} panel"),
        description: None,
        size,
        visualization: VisualizationIntent {
            visualization_type: visualization_type.into(),
            unit: None,
            reducer: None,
            decimals: None,
            min: None,
            max: None,
            threshold_mode: None,
            thresholds: Vec::new(),
            legend: None,
        },
        queries: vec![query],
        transparent: None,
    }
}

fn query(kind: &str, visualization_type: &str) -> AuthoringQuery {
    match kind {
        "promql" => AuthoringQuery::Promql {
            expression: "sum(rate(http_requests_total[5m]))".into(),
            stream: None,
            legend: Some("requests".into()),
            format: Some("time_series".into()),
            step: Some("30s".into()),
        },
        "sql" => AuthoringQuery::Sql {
            stream: "application_logs".into(),
            statement: "SELECT timestamp, count(*) FROM application_logs GROUP BY timestamp".into(),
            time_column: Some("timestamp".into()),
            legend: None,
            format: Some(
                match visualization_type {
                    "logs" => "logs",
                    "time_series" => "time_series",
                    _ => "table",
                }
                .into(),
            ),
        },
        "trace" => AuthoringQuery::Trace {
            stream: "application_traces".into(),
            query: "service.name = 'api'".into(),
            limit: Some(100),
            legend: None,
        },
        "profile" => AuthoringQuery::Profile {
            stream: "application_profiles".into(),
            query: "service.name = 'api'".into(),
            profile_type: "cpu".into(),
            aggregate: Some("sum".into()),
            legend: None,
        },
        other => panic!("unsupported test query kind {other}"),
    }
}
