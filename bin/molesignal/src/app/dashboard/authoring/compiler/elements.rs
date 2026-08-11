// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde_json::{Map, Value, json};

use super::{
    CompileContext,
    layout::{LayoutCursor, panel_dimensions, text_dimensions},
    queries::compile_query,
};
use crate::{
    domain::dashboard::authoring::{
        AuthoringElement, AuthoringSection, PanelAuthoringSpec, SectionElement, TextAuthoringSpec,
        VisualizationManifest,
    },
    shared::{Error, Result, contracts::ContractIssue},
};

pub(super) fn compile_elements(
    elements: &[AuthoringElement],
    context: &mut CompileContext,
    manifest: &VisualizationManifest,
) -> Result<Vec<Value>> {
    let mut cursor = LayoutCursor::default();
    elements
        .iter()
        .map(|element| match element {
            AuthoringElement::Panel(panel) => compile_panel(panel, &mut cursor, context, manifest),
            AuthoringElement::Text(text) => Ok(compile_text(text, &mut cursor, context, manifest)),
            AuthoringElement::Section(section) => {
                compile_section(section, &mut cursor, context, manifest)
            }
        })
        .collect()
}

fn compile_section(
    section: &AuthoringSection,
    parent_cursor: &mut LayoutCursor,
    context: &mut CompileContext,
    manifest: &VisualizationManifest,
) -> Result<Value> {
    let id = context.element_id("section");
    let mut child_cursor = LayoutCursor::default();
    let children = section
        .elements
        .iter()
        .map(|element| match element {
            SectionElement::Panel(panel) => {
                compile_panel(panel, &mut child_cursor, context, manifest)
            }
            SectionElement::Text(text) => {
                Ok(compile_text(text, &mut child_cursor, context, manifest))
            }
        })
        .collect::<Result<Vec<_>>>()?;
    let dimensions = crate::domain::dashboard::authoring::GridDimensions {
        w: manifest.grid.columns,
        h: child_cursor.bottom().max(8),
    };
    let mut compiled = json!({
        "kind": "row",
        "id": id,
        "title": section.title,
        "gridPos": parent_cursor.place(dimensions, manifest.grid.columns),
        "collapsed": section.collapsed,
        "elements": children
    });
    if let Some(description) = &section.description {
        compiled["description"] = Value::String(description.clone());
    }
    Ok(compiled)
}

fn compile_text(
    text: &TextAuthoringSpec,
    cursor: &mut LayoutCursor,
    context: &mut CompileContext,
    manifest: &VisualizationManifest,
) -> Value {
    let mut compiled = json!({
        "kind": "text",
        "id": context.element_id("text"),
        "title": text.title.as_deref().unwrap_or(""),
        "gridPos": cursor.place(text_dimensions(text.size, manifest), manifest.grid.columns),
        "content": text.content,
        "mode": text.mode.map_or("markdown", |mode| mode.as_str()),
        "transparent": text.transparent.unwrap_or(false)
    });
    if text.title.is_none() {
        compiled["title"] = Value::String(String::new());
    }
    compiled
}

fn compile_panel(
    panel: &PanelAuthoringSpec,
    cursor: &mut LayoutCursor,
    context: &mut CompileContext,
    manifest: &VisualizationManifest,
) -> Result<Value> {
    let Some(capability) = manifest
        .visualization(&panel.visualization.visualization_type)
        .filter(|capability| capability.visualization_type != "text")
    else {
        return Err(Error::validation(
            "dashboard authoring specification is invalid",
            vec![ContractIssue::new(
                "UNSUPPORTED_VISUALIZATION",
                "/elements",
                format!(
                    "unsupported panel visualization: {}",
                    panel.visualization.visualization_type
                ),
                true,
            )],
        ));
    };
    let mut options = capability.default_options.clone();
    merge_visualization_options(&mut options, panel);
    let field_config = compile_field_config(panel);
    let queries = panel
        .queries
        .iter()
        .enumerate()
        .map(|(index, query)| {
            let compiled = compile_query(query);
            let mut value = json!({
                "refId": ref_id(index),
                "enabled": true,
                "dataSourceType": compiled.data_source_type,
                "query": compiled.payload,
                "format": compiled.format
            });
            if let Some(legend) = compiled.legend {
                value["legend"] = Value::String(legend);
            }
            value
        })
        .collect::<Vec<_>>();
    let mut compiled = json!({
        "kind": "panel",
        "id": context.element_id("panel"),
        "title": panel.title,
        "gridPos": cursor.place(
            panel_dimensions(panel.size, capability, manifest),
            manifest.grid.columns
        ),
        "queryOptions": { "timeoutMs": manifest.limits.preflight_timeout_ms },
        "queries": queries,
        "transformations": [],
        "visualization": {
            "type": capability.visualization_type,
            "schemaVersion": capability.option_schema_version,
            "options": options
        },
        "fieldConfig": field_config,
        "overrides": [],
        "links": [],
        "transparent": panel.transparent.unwrap_or(false)
    });
    if let Some(description) = &panel.description {
        compiled["description"] = Value::String(description.clone());
    }
    Ok(compiled)
}

fn merge_visualization_options(options: &mut Map<String, Value>, panel: &PanelAuthoringSpec) {
    let visualization = &panel.visualization;
    if let Some(reducer) = &visualization.reducer {
        options.insert("calculation".into(), Value::String(reducer.clone()));
    }
    if let Some(legend) = &visualization.legend {
        if let Some(mode) = legend.mode {
            options.insert("legendMode".into(), Value::String(mode.as_str().into()));
        }
        if let Some(placement) = legend.placement {
            options.insert(
                "legendPlacement".into(),
                Value::String(placement.as_str().into()),
            );
        }
        if !legend.stats.is_empty() {
            options.insert("legendStats".into(), json!(legend.stats));
        }
    }
}

fn compile_field_config(panel: &PanelAuthoringSpec) -> Value {
    let visualization = &panel.visualization;
    let mut config = Map::new();
    if let Some(unit) = &visualization.unit
        && unit != "none"
    {
        config.insert("unit".into(), Value::String(unit.clone()));
    }
    if let Some(decimals) = visualization.decimals {
        config.insert("decimals".into(), Value::from(decimals));
    }
    if let Some(minimum) = visualization.min {
        config.insert("min".into(), Value::from(minimum));
    }
    if let Some(maximum) = visualization.max {
        config.insert("max".into(), Value::from(maximum));
    }
    if !visualization.thresholds.is_empty() {
        config.insert(
            "thresholds".into(),
            json!({
                "mode": visualization.threshold_mode.map_or("absolute", |mode| mode.as_str()),
                "steps": visualization.thresholds
            }),
        );
    }
    Value::Object(config)
}

fn ref_id(index: usize) -> String {
    char::from(b'A'.saturating_add(index.min(25) as u8)).to_string()
}
