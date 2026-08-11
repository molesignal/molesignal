// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde_json::{Value, json};

use super::{CompileContext, queries::compile_query};
use crate::{domain::dashboard::authoring::AuthoringVariable, shared::Result};

pub(super) fn compile_variables(
    variables: &[AuthoringVariable],
    context: &mut CompileContext,
) -> Result<Vec<Value>> {
    Ok(variables
        .iter()
        .map(|variable| compile_variable(variable, context))
        .collect())
}

fn compile_variable(variable: &AuthoringVariable, context: &mut CompileContext) -> Value {
    let id = context.variable_id();
    match variable {
        AuthoringVariable::Custom {
            name,
            label,
            values,
            default_value,
            multi,
            include_all,
        } => {
            let selected = default_value.as_ref().or_else(|| values.first());
            json!({
                "id": id,
                "name": name,
                "label": label.as_deref().unwrap_or(name),
                "type": "custom",
                "options": values.iter().map(|value| json!({
                    "label": scalar_label(value),
                    "value": value,
                    "selected": selected == Some(value)
                })).collect::<Vec<_>>(),
                "defaultValue": selected,
                "currentValue": selected,
                "multi": multi,
                "includeAll": include_all,
                "hide": "none",
                "refresh": "never"
            })
        }
        AuthoringVariable::Query {
            name,
            label,
            query,
            default_value,
            multi,
            include_all,
            refresh,
        } => {
            let query = compile_query(query);
            json!({
                "id": id,
                "name": name,
                "label": label.as_deref().unwrap_or(name),
                "type": "query",
                "query": query.payload,
                "defaultValue": default_value,
                "currentValue": default_value,
                "multi": multi,
                "includeAll": include_all,
                "hide": "none",
                "refresh": refresh.as_deref().unwrap_or("dashboard_load")
            })
        }
        AuthoringVariable::Constant {
            name,
            label,
            value,
            hidden,
        }
        | AuthoringVariable::Text {
            name,
            label,
            value,
            hidden,
        } => json!({
            "id": id,
            "name": name,
            "label": label.as_deref().unwrap_or(name),
            "type": if matches!(variable, AuthoringVariable::Constant { .. }) { "constant" } else { "text" },
            "defaultValue": value,
            "currentValue": value,
            "multi": false,
            "includeAll": false,
            "hide": if *hidden { "variable" } else { "none" },
            "refresh": "never"
        }),
        AuthoringVariable::Interval {
            name,
            label,
            values,
            default_value,
        } => {
            let selected = default_value.as_ref().or_else(|| values.first());
            json!({
                "id": id,
                "name": name,
                "label": label.as_deref().unwrap_or(name),
                "type": "interval",
                "options": values.iter().map(|value| json!({
                    "label": value,
                    "value": value,
                    "selected": selected == Some(value)
                })).collect::<Vec<_>>(),
                "defaultValue": selected,
                "currentValue": selected,
                "multi": false,
                "includeAll": false,
                "hide": "none",
                "refresh": "never"
            })
        }
        AuthoringVariable::DataSource {
            name,
            label,
            data_source_type,
            default_value,
        } => json!({
            "id": id,
            "name": name,
            "label": label.as_deref().unwrap_or(name),
            "type": "data_source",
            "query": { "dataSourceType": data_source_type },
            "defaultValue": default_value,
            "currentValue": default_value,
            "multi": false,
            "includeAll": false,
            "hide": "none",
            "refresh": "never"
        }),
    }
}

fn scalar_label(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        scalar => scalar.to_string(),
    }
}
