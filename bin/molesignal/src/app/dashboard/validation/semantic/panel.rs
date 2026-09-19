// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::HashSet;

use serde_json::{Map, Value};

use super::{SemanticState, references::collect_variable_references};
use crate::domain::dashboard::authoring::VisualizationCapability;

pub(super) fn validate_panel(
    panel: &Map<String, Value>,
    panel_id: &str,
    path: &str,
    state: &mut SemanticState<'_>,
) {
    state.panel_count += 1;
    let manifest = state.manifest;
    let visualization = panel.get("visualization").and_then(Value::as_object);
    let visualization_type = visualization
        .and_then(|value| value.get("type"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let capability = manifest.visualization(visualization_type);
    if capability.is_none() {
        state.issue(
            "UNSUPPORTED_VISUALIZATION",
            format!("{path}/visualization/type"),
            format!("unsupported visualization: {visualization_type}"),
        );
    }
    if let (Some(config), Some(capability)) = (visualization, capability) {
        if config.get("schemaVersion").and_then(Value::as_u64)
            != Some(u64::from(capability.option_schema_version))
        {
            state.issue(
                "UNSUPPORTED_VISUALIZATION_VERSION",
                format!("{path}/visualization/schemaVersion"),
                "visualization option schema version is not supported",
            );
        }
        validate_visualization_semantics(panel, config, capability, path, state);
    }

    let queries = panel
        .get("queries")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    if queries.is_empty() && visualization_type != "text" {
        state.issue(
            "EMPTY_PANEL_QUERIES",
            format!("{path}/queries"),
            "non-text panels require at least one query",
        );
    }
    if queries.len() > manifest.limits.max_queries_per_panel {
        state.issue(
            "QUERY_BUDGET_EXCEEDED",
            format!("{path}/queries"),
            format!(
                "panel has {} queries; maximum is {}",
                queries.len(),
                manifest.limits.max_queries_per_panel
            ),
        );
    }
    let mut refs = HashSet::new();
    for (index, query) in queries.iter().enumerate() {
        let query_path = format!("{path}/queries/{index}");
        let ref_id = query
            .get("refId")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if !refs.insert(ref_id.to_string()) {
            state.issue(
                "DUPLICATE_QUERY_REF_ID",
                format!("{query_path}/refId"),
                format!("duplicate query refId: {ref_id}"),
            );
        }
        validate_query(query, capability, &query_path, state);
        if let Some(shared) = query.get("sharedQuery").and_then(Value::as_object)
            && let (Some(source_panel), Some(source_ref)) = (
                shared.get("sourcePanelId").and_then(Value::as_str),
                shared.get("sourceRefId").and_then(Value::as_str),
            )
        {
            state.shared_queries.push((
                format!("{query_path}/sharedQuery"),
                source_panel.to_string(),
                source_ref.to_string(),
            ));
        }
    }
    state.panel_refs.insert(panel_id.to_string(), refs);
}

fn validate_visualization_semantics(
    panel: &Map<String, Value>,
    visualization: &Map<String, Value>,
    capability: &VisualizationCapability,
    path: &str,
    state: &mut SemanticState<'_>,
) {
    if let Some(unit) = panel
        .get("fieldConfig")
        .and_then(Value::as_object)
        .and_then(|config| config.get("unit"))
        .and_then(Value::as_str)
        && !capability
            .allowed_units
            .iter()
            .any(|allowed| allowed == unit)
    {
        state.issue(
            "UNSUPPORTED_VISUALIZATION_UNIT",
            format!("{path}/fieldConfig/unit"),
            format!("unit {unit} is not supported by this visualization"),
        );
    }
    if let Some(reducer) = visualization
        .get("options")
        .and_then(Value::as_object)
        .and_then(|options| options.get("calculation"))
        .and_then(Value::as_str)
        && !capability.allowed_reducers.is_empty()
        && !capability
            .allowed_reducers
            .iter()
            .any(|allowed| allowed == reducer)
    {
        state.issue(
            "UNSUPPORTED_VISUALIZATION_REDUCER",
            format!("{path}/visualization/options/calculation"),
            format!("reducer {reducer} is not supported by this visualization"),
        );
    }
}

fn validate_query(
    query: &Value,
    capability: Option<&VisualizationCapability>,
    path: &str,
    state: &mut SemanticState<'_>,
) {
    let Some(config) = query.get("query").and_then(Value::as_object) else {
        return;
    };
    let source = query
        .get("dataSourceType")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let (kind, default_shape, expression_required) = match source {
        "metrics" => ("promql", "time_series", true),
        "logs" => ("sql", "logs", true),
        "sql" => ("sql", "table", true),
        "traces" => ("trace", "traces", true),
        "profiles" => ("profile", "profiles", false),
        _ => return,
    };
    if let Some(language) = config.get("language").and_then(Value::as_str) {
        let expected = if kind == "promql" { "promql" } else { "sql" };
        if kind != "profile" && kind != "trace" && language != expected {
            state.issue(
                "INVALID_QUERY_LANGUAGE",
                format!("{path}/query/language"),
                format!("{source} queries require {expected}"),
            );
        }
    }
    let expression = first_string(config, &["expression", "statement", "sql", "query"]);
    if expression_required && expression.is_none_or(|value| value.trim().is_empty()) {
        state.issue(
            "INVALID_TYPED_QUERY",
            format!("{path}/query"),
            format!("{kind} query text must not be empty"),
        );
    }
    let query_length = string_bytes(config);
    if query_length > state.manifest.limits.max_query_length {
        state.issue(
            "QUERY_LENGTH_EXCEEDED",
            format!("{path}/query"),
            format!(
                "query contains {query_length} bytes; maximum is {}",
                state.manifest.limits.max_query_length
            ),
        );
    }
    if kind == "profile" && first_string(config, &["profileType", "type"]).is_none_or(str::is_empty)
    {
        state.issue(
            "INVALID_TYPED_QUERY",
            format!("{path}/query/profileType"),
            "profile query requires profileType",
        );
    }
    if let Some(capability) = capability {
        validate_compatibility(query, kind, default_shape, capability, path, state);
    }
    collect_variable_references(config, &format!("{path}/query"), state);
}

fn validate_compatibility(
    query: &Value,
    kind: &str,
    default_shape: &str,
    capability: &VisualizationCapability,
    path: &str,
    state: &mut SemanticState<'_>,
) {
    if !capability
        .compatible_query_kinds
        .iter()
        .any(|candidate| candidate == kind)
    {
        state.issue(
            "INCOMPATIBLE_VISUALIZATION_QUERY",
            path,
            format!(
                "visualization {} does not support {kind} queries",
                capability.visualization_type
            ),
        );
    }
    let shape = query
        .get("format")
        .and_then(Value::as_str)
        .unwrap_or(default_shape);
    if !capability
        .compatible_data_shapes
        .iter()
        .any(|candidate| candidate == shape)
    {
        state.issue(
            "INCOMPATIBLE_VISUALIZATION_DATA_SHAPE",
            format!("{path}/format"),
            format!(
                "visualization {} does not support {shape} data",
                capability.visualization_type
            ),
        );
    }
}

fn first_string<'a>(object: &'a Map<String, Value>, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|key| object.get(*key).and_then(Value::as_str))
}

fn string_bytes(value: &Map<String, Value>) -> usize {
    value.values().filter_map(Value::as_str).map(str::len).sum()
}
