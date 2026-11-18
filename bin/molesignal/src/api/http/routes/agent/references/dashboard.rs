// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! A dashboard attached to a question: its title, tags, and the title and first query of each
//! panel, which tell the model what the dashboard looks at.

use domain::dashboard::Dashboard;
use serde_json::Value;

/// The most panels the model is told of.
const MOST_PANELS: usize = 30;

pub(super) fn dashboard_facts(dashboard: &Dashboard) -> String {
    // The titles and queries of the panels tell the model what the dashboard looks at.
    let mut panels = Vec::new();

    if let Some(elements) = dashboard.model.get("elements").and_then(Value::as_array) {
        collect_panels(elements, &mut panels);
    }

    let panels = panels
        .into_iter()
        .take(MOST_PANELS)
        .map(panel_line)
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        "Dashboard: {}\nTags: {}\nPanels:\n{}",
        dashboard.title,
        dashboard.tags.join(", "),
        panels
    )
}

/// The panels of `elements` in reading order, those inside groups, rows and tabs included.
fn collect_panels<'a>(elements: &'a [Value], panels: &mut Vec<&'a Value>) {
    for element in elements {
        if element.get("kind").and_then(Value::as_str) == Some("panel") {
            panels.push(element);
        }

        if let Some(children) = element.get("elements").and_then(Value::as_array) {
            collect_panels(children, panels);
        }

        let tabs = element.get("tabs").and_then(Value::as_array);

        for tab in tabs.into_iter().flatten() {
            if let Some(children) = tab.get("elements").and_then(Value::as_array) {
                collect_panels(children, panels);
            }
        }
    }
}

/// A panel as one line: its title and the text of its first query.
fn panel_line(panel: &Value) -> String {
    let title = panel
        .get("title")
        .and_then(Value::as_str)
        .filter(|title| !title.trim().is_empty())
        .unwrap_or("(untitled)");

    let query = panel
        .pointer("/queries/0/query")
        .map(query_text)
        .unwrap_or_default();

    if query.is_empty() {
        format!("- {title}")
    } else {
        format!("- {title}: {query}")
    }
}

/// What a panel query asks: its SQL statement, PromQL expression or filter, else the query as
/// JSON.
fn query_text(query: &Value) -> String {
    ["statement", "expression", "filter"]
        .iter()
        .find_map(|key| query.get(*key).and_then(Value::as_str))
        .map(str::to_owned)
        .unwrap_or_else(|| query.to_string())
}

#[cfg(test)]
mod tests {
    use kernel::{ids::Id, time::TimestampMicros};
    use serde_json::json;

    use super::*;

    fn dashboard(model: Value) -> Dashboard {
        Dashboard {
            id: Id::new(),
            org_id: Id::new(),
            folder_id: None,
            uid: "checkout".into(),
            title: "Checkout".into(),
            tags: vec!["shop".into()],
            model,
            version: 1,
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(0),
            created_by: Id::new(),
            updated_by: Id::new(),
        }
    }

    fn panel(title: &str, query: Value) -> Value {
        json!({
            "kind": "panel",
            "id": title.to_lowercase().replace(' ', "-"),
            "title": title,
            "queries": [{"refId": "A", "enabled": true, "dataSourceType": "sql", "query": query}],
        })
    }

    #[test]
    fn panels_are_read_from_the_elements_of_every_container() {
        let model = json!({
            "schemaVersion": 2,
            "elements": [
                {"kind": "text", "id": "about", "title": "About", "content": "Read me"},
                panel("Requests", json!({"kind": "sql", "statement": "SELECT count(*) FROM logs"})),
                {
                    "kind": "group",
                    "id": "latency",
                    "title": "Latency",
                    "elements": [panel("p95", json!({"kind": "promql", "expression": "histogram_quantile(0.95, x)"}))],
                },
                {
                    "kind": "tab",
                    "id": "views",
                    "tabs": [{
                        "id": "profiles",
                        "title": "Profiles",
                        "elements": [panel("CPU", json!({"kind": "profile", "filter": "service = 'cart'"}))],
                    }],
                },
            ],
        });

        assert_eq!(
            dashboard_facts(&dashboard(model)),
            "Dashboard: Checkout\nTags: shop\nPanels:\n\
             - Requests: SELECT count(*) FROM logs\n\
             - p95: histogram_quantile(0.95, x)\n\
             - CPU: service = 'cart'"
        );
    }

    #[test]
    fn a_query_without_text_is_given_as_json_and_a_panel_without_one_by_its_title() {
        let mut bare = panel("Bare", Value::Null);
        bare.as_object_mut().expect("panel").remove("queries");

        let model = json!({
            "elements": [panel("Odd", json!({"kind": "custom", "q": 1})), bare],
        });

        assert_eq!(
            dashboard_facts(&dashboard(model)),
            "Dashboard: Checkout\nTags: shop\nPanels:\n\
             - Odd: {\"kind\":\"custom\",\"q\":1}\n\
             - Bare"
        );
    }

    #[test]
    fn at_most_thirty_panels_are_told() {
        let elements = (0..40)
            .map(|index| panel(&format!("P{index}"), json!({"statement": "SELECT 1"})))
            .collect::<Vec<_>>();

        let facts = dashboard_facts(&dashboard(json!({ "elements": elements })));

        assert_eq!(
            facts.lines().filter(|line| line.starts_with("- ")).count(),
            MOST_PANELS
        );
    }
}
