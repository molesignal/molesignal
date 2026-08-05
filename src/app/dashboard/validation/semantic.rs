// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::{HashMap, HashSet};

use serde_json::{Map, Value};

use crate::{
    domain::dashboard::authoring::{VisualizationManifest, visualization_manifest},
    shared::contracts::ContractIssue,
};

mod panel;
mod references;

use panel::validate_panel;
use references::validate_references;

const MAX_SEMANTIC_ISSUES: usize = 50;

#[derive(Debug, Clone, Copy)]
struct Rect {
    x: i64,
    y: i64,
    w: i64,
    h: i64,
}

struct SemanticState<'a> {
    manifest: &'a VisualizationManifest,
    issues: Vec<ContractIssue>,
    ids: HashSet<String>,
    variable_names: HashSet<String>,
    variable_references: Vec<(String, String)>,
    panel_refs: HashMap<String, HashSet<String>>,
    shared_queries: Vec<(String, String, String)>,
    element_count: usize,
    panel_count: usize,
}

impl<'a> SemanticState<'a> {
    fn new(manifest: &'a VisualizationManifest) -> Self {
        Self {
            manifest,
            issues: Vec::new(),
            ids: HashSet::new(),
            variable_names: HashSet::new(),
            variable_references: Vec::new(),
            panel_refs: HashMap::new(),
            shared_queries: Vec::new(),
            element_count: 0,
            panel_count: 0,
        }
    }

    fn issue(&mut self, code: &str, path: impl Into<String>, message: impl Into<String>) {
        if self.issues.len() < MAX_SEMANTIC_ISSUES {
            self.issues
                .push(ContractIssue::new(code, path, message, true));
        }
    }
}

#[must_use]
pub fn validate_dashboard_semantics(model: &Value) -> Vec<ContractIssue> {
    validate_dashboard_semantics_with(model, visualization_manifest())
}

#[must_use]
pub fn validate_dashboard_semantics_with(
    model: &Value,
    manifest: &VisualizationManifest,
) -> Vec<ContractIssue> {
    let Some(root) = model.as_object() else {
        return vec![ContractIssue::new(
            "CONTRACT_TYPE",
            "",
            "dashboard model must be an object",
            true,
        )];
    };
    let mut state = SemanticState::new(manifest);
    validate_refresh(root.get("refreshSettings"), &mut state);
    validate_variables(root.get("variables"), &mut state);
    let columns = root
        .get("layout")
        .and_then(Value::as_object)
        .and_then(|layout| layout.get("columns"))
        .and_then(Value::as_i64)
        .unwrap_or(24);
    if let Some(elements) = root.get("elements").and_then(Value::as_array) {
        validate_elements(elements, "/elements", columns, &mut state);
    }
    validate_references(&mut state);

    let max_elements = state.manifest.limits.max_elements;
    let max_panels = state.manifest.limits.max_panels;
    if state.element_count > max_elements {
        state.issue(
            "ELEMENT_BUDGET_EXCEEDED",
            "/elements",
            format!(
                "dashboard has {} elements; maximum is {}",
                state.element_count, max_elements
            ),
        );
    }
    if state.panel_count > max_panels {
        state.issue(
            "PANEL_BUDGET_EXCEEDED",
            "/elements",
            format!(
                "dashboard has {} panels; maximum is {}",
                state.panel_count, max_panels
            ),
        );
    }
    state.issues
}

fn validate_refresh(value: Option<&Value>, state: &mut SemanticState<'_>) {
    let Some(refresh) = value.and_then(Value::as_object) else {
        return;
    };
    let enabled = refresh.get("enabled").and_then(Value::as_bool);
    let mode = refresh.get("mode").and_then(Value::as_str);
    match mode {
        Some("off") if enabled != Some(false) => state.issue(
            "INVALID_REFRESH_COMBINATION",
            "/refreshSettings/enabled",
            "refresh mode off requires enabled=false",
        ),
        Some("interval") => {
            if enabled != Some(true) {
                state.issue(
                    "INVALID_REFRESH_COMBINATION",
                    "/refreshSettings/enabled",
                    "refresh mode interval requires enabled=true",
                );
            }
            let interval = refresh
                .get("defaultInterval")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty());
            if interval.is_none() {
                state.issue(
                    "INVALID_REFRESH_COMBINATION",
                    "/refreshSettings/defaultInterval",
                    "interval refresh requires a defaultInterval",
                );
            } else if !refresh
                .get("allowedIntervals")
                .and_then(Value::as_array)
                .is_some_and(|allowed| allowed.iter().any(|value| value.as_str() == interval))
            {
                state.issue(
                    "INVALID_REFRESH_COMBINATION",
                    "/refreshSettings/defaultInterval",
                    "defaultInterval must be listed in allowedIntervals",
                );
            }
        }
        Some("live") if enabled != Some(true) => state.issue(
            "INVALID_REFRESH_COMBINATION",
            "/refreshSettings/enabled",
            "refresh mode live requires enabled=true",
        ),
        _ => {}
    }
}

fn validate_variables(value: Option<&Value>, state: &mut SemanticState<'_>) {
    let Some(variables) = value.and_then(Value::as_array) else {
        return;
    };
    for variable in variables {
        if let Some(name) = variable.get("name").and_then(Value::as_str)
            && !state.variable_names.insert(name.to_string())
        {
            state.issue(
                "DUPLICATE_VARIABLE_NAME",
                "/variables",
                format!("duplicate variable name: {name}"),
            );
        }
        if let Some(id) = variable.get("id").and_then(Value::as_str) {
            register_id(id, "/variables", state);
        }
    }
    for (index, variable) in variables.iter().enumerate() {
        if let Some(depends_on) = variable.get("dependsOn").and_then(Value::as_array) {
            for (dependency_index, dependency) in depends_on.iter().enumerate() {
                if let Some(name) = dependency.as_str()
                    && !state.variable_names.contains(name)
                {
                    state.issue(
                        "UNKNOWN_VARIABLE",
                        format!("/variables/{index}/dependsOn/{dependency_index}"),
                        format!("unknown variable dependency: {name}"),
                    );
                }
            }
        }
    }
}

fn validate_elements(elements: &[Value], path: &str, columns: i64, state: &mut SemanticState<'_>) {
    let mut occupied: Vec<(Rect, String)> = Vec::new();
    for (index, element) in elements.iter().enumerate() {
        state.element_count += 1;
        let element_path = format!("{path}/{index}");
        let Some(object) = element.as_object() else {
            continue;
        };
        let id = object.get("id").and_then(Value::as_str).unwrap_or_default();
        register_id(id, &format!("{element_path}/id"), state);
        if let Some(rect) = validate_grid_position(object, &element_path, columns, state) {
            for (other, other_path) in &occupied {
                if overlaps(rect, *other) {
                    state.issue(
                        "OVERLAPPING_GRID_POSITION",
                        format!("{element_path}/gridPos"),
                        format!("element overlaps {other_path}"),
                    );
                    break;
                }
            }
            occupied.push((rect, element_path.clone()));
        }
        match object.get("kind").and_then(Value::as_str) {
            Some("panel") => validate_panel(object, id, &element_path, state),
            Some("group" | "row") => {
                if let Some(children) = object.get("elements").and_then(Value::as_array) {
                    validate_elements(
                        children,
                        &format!("{element_path}/elements"),
                        columns,
                        state,
                    );
                }
            }
            Some("tab") => validate_tabs(object, &element_path, columns, state),
            _ => {}
        }
    }
}

fn validate_tabs(
    tab: &Map<String, Value>,
    path: &str,
    columns: i64,
    state: &mut SemanticState<'_>,
) {
    let Some(tabs) = tab.get("tabs").and_then(Value::as_array) else {
        return;
    };
    let mut tab_ids = HashSet::new();
    for (index, item) in tabs.iter().enumerate() {
        let item_path = format!("{path}/tabs/{index}");
        if let Some(id) = item.get("id").and_then(Value::as_str) {
            if !tab_ids.insert(id) {
                state.issue(
                    "DUPLICATE_TAB_ID",
                    format!("{item_path}/id"),
                    format!("duplicate tab ID: {id}"),
                );
            }
            register_id(id, &format!("{item_path}/id"), state);
        }
        if let Some(children) = item.get("elements").and_then(Value::as_array) {
            validate_elements(children, &format!("{item_path}/elements"), columns, state);
        }
    }
    if let Some(default_id) = tab.get("defaultTabId").and_then(Value::as_str)
        && !tab_ids.contains(default_id)
    {
        state.issue(
            "UNKNOWN_DEFAULT_TAB",
            format!("{path}/defaultTabId"),
            "defaultTabId does not reference a tab",
        );
    }
}

fn validate_grid_position(
    object: &Map<String, Value>,
    path: &str,
    columns: i64,
    state: &mut SemanticState<'_>,
) -> Option<Rect> {
    let grid = object.get("gridPos")?.as_object()?;
    let rect = Rect {
        x: grid.get("x")?.as_i64()?,
        y: grid.get("y")?.as_i64()?,
        w: grid.get("w")?.as_i64()?,
        h: grid.get("h")?.as_i64()?,
    };
    if rect.x < 0 || rect.y < 0 || rect.w < 1 || rect.h < 1 || rect.x + rect.w > columns {
        state.issue(
            "INVALID_GRID_POSITION",
            format!("{path}/gridPos"),
            format!("element must fit within the {columns}-column grid"),
        );
    }
    for (min_key, max_key, actual) in [("minW", "maxW", rect.w), ("minH", "maxH", rect.h)] {
        let minimum = grid.get(min_key).and_then(Value::as_i64);
        let maximum = grid.get(max_key).and_then(Value::as_i64);
        if minimum.is_some_and(|value| value > actual)
            || maximum.is_some_and(|value| value < actual)
            || minimum.zip(maximum).is_some_and(|(min, max)| min > max)
        {
            state.issue(
                "INVALID_GRID_CONSTRAINT",
                format!("{path}/gridPos"),
                format!("{min_key}/{max_key} conflict with the element size"),
            );
        }
    }
    Some(rect)
}

fn register_id(id: &str, path: &str, state: &mut SemanticState<'_>) {
    if !state.ids.insert(id.to_string()) {
        state.issue(
            "DUPLICATE_ELEMENT_ID",
            path,
            format!("duplicate recursive identifier: {id}"),
        );
    }
}

fn overlaps(left: Rect, right: Rect) -> bool {
    left.x < right.x + right.w
        && left.x + left.w > right.x
        && left.y < right.y + right.h
        && left.y + left.h > right.y
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;

    const VALID: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/contracts/dashboard/fixtures/valid/dashboard-v2-nested.json"
    ));
    const DUPLICATE: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/contracts/dashboard/fixtures/invalid/dashboard-v2-duplicate-id.json"
    ));

    #[test]
    fn validates_recursive_ids_and_current_query_shapes() {
        let valid: Value = serde_json::from_str(VALID).unwrap();
        assert!(validate_dashboard_semantics(&valid).is_empty());

        let duplicate: Value = serde_json::from_str(DUPLICATE).unwrap();
        assert!(
            validate_dashboard_semantics(&duplicate)
                .iter()
                .any(|issue| issue.code == "DUPLICATE_ELEMENT_ID")
        );
    }

    #[test]
    fn variable_scanner_ignores_query_builtins() {
        assert_eq!(
            references::variable_names("$service ${region} $__from $__to"),
            vec!["service", "region"]
        );
    }
}
