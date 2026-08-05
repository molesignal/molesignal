// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde_json::{Map, Value};

use super::SemanticState;

pub(super) fn validate_references(state: &mut SemanticState<'_>) {
    let variable_references = std::mem::take(&mut state.variable_references);
    for (path, variable) in variable_references {
        if !state.variable_names.contains(&variable) {
            state.issue(
                "UNKNOWN_VARIABLE",
                path,
                format!("query references unknown variable: {variable}"),
            );
        }
    }
    let shared_queries = std::mem::take(&mut state.shared_queries);
    for (path, panel, ref_id) in shared_queries {
        if !state
            .panel_refs
            .get(&panel)
            .is_some_and(|refs| refs.contains(&ref_id))
        {
            state.issue(
                "UNKNOWN_SHARED_QUERY",
                path,
                format!("shared query references unknown {panel}:{ref_id}"),
            );
        }
    }
}

pub(super) fn collect_variable_references(
    value: &Map<String, Value>,
    path: &str,
    state: &mut SemanticState<'_>,
) {
    collect_from_value(&Value::Object(value.clone()), path, state);
}

fn collect_from_value(value: &Value, path: &str, state: &mut SemanticState<'_>) {
    match value {
        Value::String(text) => {
            for variable in variable_names(text) {
                state.variable_references.push((path.to_string(), variable));
            }
        }
        Value::Array(values) => {
            for (index, value) in values.iter().enumerate() {
                collect_from_value(value, &format!("{path}/{index}"), state);
            }
        }
        Value::Object(values) => {
            for (key, value) in values {
                collect_from_value(value, &format!("{path}/{}", pointer_segment(key)), state);
            }
        }
        _ => {}
    }
}

pub(super) fn variable_names(text: &str) -> Vec<String> {
    let chars = text.chars().collect::<Vec<_>>();
    let mut names = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        if chars[index] != '$' {
            index += 1;
            continue;
        }
        index += 1;
        let braced = chars.get(index) == Some(&'{');
        if braced {
            index += 1;
        }
        let start = index;
        while chars
            .get(index)
            .is_some_and(|character| character.is_ascii_alphanumeric() || *character == '_')
        {
            index += 1;
        }
        if index > start {
            let name = chars[start..index].iter().collect::<String>();
            if !name.starts_with("__") {
                names.push(name);
            }
        }
        if braced && chars.get(index) == Some(&'}') {
            index += 1;
        }
    }
    names
}

fn pointer_segment(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
}
