// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! An event as the row of its stream: nested objects become dotted columns, typed as the stream
//! declares them. What no column holds is not lost: it goes to `extra`, or to `usr.extra` for the
//! custom attributes of a user.

use std::{collections::HashMap, sync::LazyLock};

use serde_json::{Map, Value};

use super::{
    address,
    convert::{column_value, truncate},
};
use crate::domain::{
    rum::event::{RumField, RumStream, field, fields},
    stream::FieldType,
};

#[derive(Default)]
struct Node {
    /// The column that holds the value at this path (a scalar, or a whole JSON object).
    column: Option<&'static RumField>,
    /// The column that collects what is found under this path without a column of its own.
    catch_all: Option<&'static RumField>,
    children: HashMap<&'static str, Node>,
}

static TRIES: LazyLock<[Node; 8]> =
    LazyLock::new(|| std::array::from_fn(|i| build(RumStream::ALL[i])));

fn build(stream: RumStream) -> Node {
    let mut root = Node::default();

    for field in fields(stream) {
        let mut path = field.path().collect::<Vec<_>>();

        if field.is_catch_all() {
            path.pop();
            descend(&mut root, &path).catch_all = Some(field);
        } else {
            descend(&mut root, &path).column = Some(field);
        }
    }

    root
}

fn descend<'a>(mut node: &'a mut Node, path: &[&'static str]) -> &'a mut Node {
    for segment in path {
        node = node.children.entry(segment).or_default();
    }

    node
}

/// The row an event becomes in `stream`.
pub fn flatten(stream: RumStream, event: Map<String, Value>) -> Map<String, Value> {
    let root = &TRIES[stream as usize];
    let mut row = Map::new();
    let unknown = walk(root, event, &mut row);

    keep_unknown(root, unknown, &mut row);
    derive(stream, &mut row);

    row
}

fn walk(
    node: &Node,
    object: Map<String, Value>,
    row: &mut Map<String, Value>,
) -> Map<String, Value> {
    let mut unknown = Map::new();

    for (key, value) in object {
        let Some(child) = node.children.get(key.as_str()) else {
            unknown.insert(key, value);
            continue;
        };

        if let Some(column) = child.column {
            match column_value(column.data_type, column.max_len, &value) {
                Some(converted) => {
                    row.insert(column.name.to_string(), converted);
                }
                None if value.is_null() => {}
                None => {
                    unknown.insert(key, value);
                }
            }

            continue;
        }

        match value {
            Value::Object(inner) => {
                let rest = walk(child, inner, row);

                if rest.is_empty() {
                    continue;
                }

                if child.catch_all.is_some() {
                    keep_unknown(child, rest, row);
                } else {
                    unknown.insert(key, Value::Object(rest));
                }
            }
            Value::Null => {}
            other => {
                unknown.insert(key, other);
            }
        }
    }

    unknown
}

fn keep_unknown(node: &Node, unknown: Map<String, Value>, row: &mut Map<String, Value>) {
    let Some(column) = node.catch_all else {
        return;
    };

    if unknown.is_empty() {
        return;
    }

    if let Some(kept) = column_value(column.data_type, column.max_len, &Value::Object(unknown)) {
        row.insert(column.name.to_string(), kept);
    }
}

/// The columns that are worked out from others: the parts of a URL a page groups by, how many
/// frustration signals an action carries, and the script a long task is put down to.
fn derive(stream: RumStream, row: &mut Map<String, Value>) {
    derive_url(row, "view.url", "view");

    if stream == RumStream::Resources {
        derive_url(row, "resource.url", "resource");
    }

    if stream == RumStream::Actions {
        let signals = row
            .get("action.frustration.type")
            .and_then(Value::as_array)
            .map_or(0, Vec::len);

        row.insert(
            "action.frustration.count".into(),
            Value::from(signals as i64),
        );
    }

    if stream == RumStream::LongTasks {
        derive_top_script(row);
    }
}

/// What the browser says of a script, as the columns of the script a long task is put down to:
/// the name of the key in the entry of `long_task.scripts` and the column it fills.
const TOP_SCRIPT: [(&str, &str); 3] = [
    ("source_url", "long_task.top_script.source_url"),
    ("source_function_name", "long_task.top_script.function"),
    ("invoker_type", "long_task.top_script.invoker_type"),
];

/// A long task is put down to the one of its scripts that ran the longest; the others are not
/// counted apart. The columns are the server's: what a client sends in them is dropped. A browser
/// that reports the frame without its scripts leaves them empty, and so does a script that has
/// nothing to say for a column (inline code has no address).
fn derive_top_script(row: &mut Map<String, Value>) {
    for (_, column) in TOP_SCRIPT {
        row.remove(column);
    }

    let longest = row
        .get("long_task.scripts")
        .and_then(Value::as_array)
        .and_then(|scripts| {
            scripts.iter().filter_map(Value::as_object).fold(
                None,
                |best: Option<(&Map<String, Value>, f64)>, script| {
                    let ran = script
                        .get("duration")
                        .and_then(Value::as_f64)
                        .unwrap_or(0.0);

                    match best {
                        Some((_, longest)) if ran <= longest => best,
                        _ => Some((script, ran)),
                    }
                },
            )
        })
        .map(|(script, _)| script);

    let Some(script) = longest else {
        return;
    };

    let mut found = Vec::new();

    for (key, column) in TOP_SCRIPT {
        let Some(text) = script
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
        else {
            continue;
        };

        let max_len = field(RumStream::LongTasks, column)
            .map_or(crate::domain::rum::event::DEFAULT_TEXT_LEN, |field| {
                field.max_len
            });

        found.push((
            column.to_string(),
            Value::String(truncate(text.to_string(), max_len)),
        ));
    }

    row.extend(found);
}

fn derive_url(row: &mut Map<String, Value>, source: &str, prefix: &str) {
    let Some(raw) = row.get(source).and_then(Value::as_str) else {
        return;
    };

    let parts = address::parts(raw);
    let group = parts.path_group();
    let host = parts.host;
    let path = parts.path;

    for (suffix, value) in [
        ("url_host", host),
        ("url_path", Some(path)),
        ("url_path_group", Some(group)),
    ] {
        let name = format!("{prefix}.{suffix}");

        let Some(value) = value else {
            continue;
        };

        let max_len = field_len(prefix, &name);

        row.insert(name, Value::String(truncate(value, max_len)));
    }
}

fn field_len(prefix: &str, name: &str) -> usize {
    let stream = if prefix == "resource" {
        RumStream::Resources
    } else {
        RumStream::Views
    };

    field(stream, name)
        .filter(|field| field.data_type == FieldType::Utf8)
        .map_or(crate::domain::rum::event::DEFAULT_TEXT_LEN, |field| {
            field.max_len
        })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn object(value: Value) -> Map<String, Value> {
        value.as_object().expect("object").clone()
    }

    #[test]
    fn nested_objects_become_dotted_columns() {
        let row = flatten(
            RumStream::Views,
            object(json!({
                "service": "web",
                "session": {"id": "s-1", "type": "user", "has_replay": true},
                "view": {
                    "id": "v-1",
                    "url": "https://shop.example.com/users/42/orders",
                    "revision": 3,
                    "time_spent": 8_200_000_000_i64,
                    "performance": {"lcp": {"timestamp": 900_000_000, "target_selector": "img.hero"}},
                    "error": {"count": 1}
                }
            })),
        );

        assert_eq!(row["service"], "web");
        assert_eq!(row["session.id"], "s-1");
        assert_eq!(row["session.has_replay"], true);
        assert_eq!(row["view.revision"], 3);
        assert_eq!(row["view.time_spent"], 8_200_000_000_i64);
        assert_eq!(row["view.performance.lcp.timestamp"], 900_000_000);
        assert_eq!(row["view.performance.lcp.target_selector"], "img.hero");
        assert_eq!(row["view.error.count"], 1);
        assert!(!row.contains_key("extra"));
    }

    #[test]
    fn a_zero_score_is_a_float_so_a_later_fraction_fits_the_same_column() {
        let row = flatten(
            RumStream::Views,
            object(json!({"view": {"id": "v", "performance": {"cls": {"score": 0}}}})),
        );

        assert_eq!(row["view.performance.cls.score"], json!(0.0));
        assert!(row["view.performance.cls.score"].is_f64());
    }

    #[test]
    fn whole_floats_fill_integer_columns() {
        let row = flatten(
            RumStream::Resources,
            object(json!({"resource": {"duration": 120_000_000.0, "status_code": 200.0}})),
        );

        assert_eq!(row["resource.duration"], 120_000_000);
        assert!(row["resource.duration"].is_i64());
        assert_eq!(row["resource.status_code"], 200);
    }

    #[test]
    fn json_columns_keep_the_whole_object() {
        let row = flatten(
            RumStream::Resources,
            object(json!({
                "context": {"plan": "pro", "nested": {"a": [1, 2]}},
                "resource": {"response": {"headers": {"content-type": "application/json"}}}
            })),
        );

        assert_eq!(row["context"]["nested"]["a"], json!([1, 2]));
        assert_eq!(
            row["resource.response.headers"]["content-type"],
            "application/json"
        );
    }

    #[test]
    fn what_no_column_holds_goes_to_extra() {
        let row = flatten(
            RumStream::Actions,
            object(json!({
                "experiment": {"arm": "b"},
                "action": {"id": "a-1", "type": "click", "gesture": "double"},
                "view": {"id": "v", "layout": "wide"}
            })),
        );

        assert_eq!(row["action.id"], "a-1");
        assert_eq!(row["extra"]["experiment"]["arm"], "b");
        assert_eq!(row["extra"]["action"]["gesture"], "double");
        assert_eq!(row["extra"]["view"]["layout"], "wide");
    }

    #[test]
    fn custom_user_attributes_go_to_usr_extra() {
        let row = flatten(
            RumStream::Views,
            object(
                json!({"usr": {"id": "u-42", "email": "ada@example.com", "plan": "pro", "seats": 5}}),
            ),
        );

        assert_eq!(row["usr.id"], "u-42");
        assert_eq!(row["usr.email"], "ada@example.com");
        assert_eq!(row["usr.extra"], json!({"plan": "pro", "seats": 5}));
        assert!(!row.contains_key("extra"));
    }

    #[test]
    fn a_value_of_the_wrong_shape_is_kept_in_extra_not_dropped() {
        let row = flatten(
            RumStream::Resources,
            object(json!({"resource": {"duration": "slow", "type": "fetch"}})),
        );

        assert_eq!(row["resource.type"], "fetch");
        assert!(!row.contains_key("resource.duration"));
        assert_eq!(row["extra"]["resource"]["duration"], "slow");
    }

    #[test]
    fn nulls_are_skipped() {
        let row = flatten(
            RumStream::Views,
            object(json!({"service": null, "view": {"id": "v", "name": null}})),
        );

        assert!(!row.contains_key("service"));
        assert!(!row.contains_key("view.name"));
        assert!(!row.contains_key("extra"));
    }

    #[test]
    fn the_parts_of_urls_are_derived() {
        let view = flatten(
            RumStream::Views,
            object(json!({"view": {"id": "v", "url": "https://Shop.example.com/users/42/orders"}})),
        );

        assert_eq!(view["view.url_host"], "shop.example.com");
        assert_eq!(view["view.url_path"], "/users/42/orders");
        assert_eq!(view["view.url_path_group"], "/users/?/orders");

        let resource = flatten(
            RumStream::Resources,
            object(json!({
                "view": {"id": "v", "url": "/checkout"},
                "resource": {"type": "fetch", "url": "https://api.example.com/carts/12345?x=1"}
            })),
        );

        assert_eq!(resource["resource.url_host"], "api.example.com");
        assert_eq!(resource["resource.url_path_group"], "/carts/?");
        assert_eq!(resource["view.url_path"], "/checkout");
        assert!(!resource.contains_key("view.url_host"));
    }

    #[test]
    fn an_action_counts_its_frustration_signals() {
        let row = flatten(
            RumStream::Actions,
            object(
                json!({"action": {"id": "a", "type": "click", "frustration": {"type": ["rage_click", "dead_click"]}}}),
            ),
        );

        assert_eq!(row["action.frustration.count"], 2);
        assert_eq!(
            row["action.frustration.type"],
            json!(["rage_click", "dead_click"])
        );

        let calm = flatten(
            RumStream::Actions,
            object(json!({"action": {"id": "a", "type": "click"}})),
        );

        assert_eq!(calm["action.frustration.count"], 0);
    }

    fn long_task(scripts: Value) -> Map<String, Value> {
        flatten(
            RumStream::LongTasks,
            object(json!({"long_task": {"id": "t", "duration": 210_000_000, "scripts": scripts}})),
        )
    }

    #[test]
    fn a_long_task_is_put_down_to_the_script_that_ran_longest() {
        let row = long_task(json!([
            {"duration": 40_000_000, "source_url": "https://shop.example.com/a.js", "source_function_name": "a", "invoker_type": "event-listener"},
            {"duration": 150_000_000, "source_url": "https://shop.example.com/checkout.js", "source_function_name": "validateAddress", "invoker_type": "user-callback"},
            {"duration": 20_000_000, "source_url": "https://shop.example.com/b.js"}
        ]));

        assert_eq!(
            row["long_task.top_script.source_url"],
            "https://shop.example.com/checkout.js"
        );
        assert_eq!(row["long_task.top_script.function"], "validateAddress");
        assert_eq!(row["long_task.top_script.invoker_type"], "user-callback");
        assert_eq!(row["long_task.scripts"].as_array().map(Vec::len), Some(3));
    }

    #[test]
    fn of_scripts_that_ran_alike_the_first_is_the_one() {
        let row = long_task(json!([
            {"duration": 80_000_000, "source_url": "https://shop.example.com/first.js"},
            {"duration": 80_000_000, "source_url": "https://shop.example.com/second.js"}
        ]));

        assert_eq!(
            row["long_task.top_script.source_url"],
            "https://shop.example.com/first.js"
        );
    }

    #[test]
    fn a_script_names_only_what_it_has_a_name_for() {
        let row = long_task(json!([
            {"duration": 90_000_000, "source_url": "", "source_function_name": "  ", "invoker_type": "classic-script"}
        ]));

        assert!(!row.contains_key("long_task.top_script.source_url"));
        assert!(!row.contains_key("long_task.top_script.function"));
        assert_eq!(row["long_task.top_script.invoker_type"], "classic-script");
    }

    #[test]
    fn a_long_task_without_scripts_has_no_top_script() {
        for scripts in [json!([]), json!(null), json!("not a list"), json!([7, "x"])] {
            let row = long_task(scripts);

            assert!(
                !row.keys()
                    .any(|name| name.starts_with("long_task.top_script.")),
                "{row:?}"
            );
        }
    }

    #[test]
    fn what_a_client_sends_as_a_top_script_is_dropped() {
        let row = flatten(
            RumStream::LongTasks,
            object(json!({"long_task": {
                "id": "t",
                "top_script": {"source_url": "https://evil.example.com/x.js", "function": "x"},
                "scripts": [{"duration": 1, "source_url": "https://shop.example.com/app.js"}]
            }})),
        );

        assert_eq!(
            row["long_task.top_script.source_url"],
            "https://shop.example.com/app.js"
        );
        assert!(!row.contains_key("long_task.top_script.function"));

        let without = flatten(
            RumStream::LongTasks,
            object(
                json!({"long_task": {"id": "t", "top_script": {"source_url": "https://evil.example.com/x.js"}}}),
            ),
        );

        assert!(!without.contains_key("long_task.top_script.source_url"));
    }

    #[test]
    fn the_address_of_a_script_is_cut_to_the_length_of_its_column() {
        let row = long_task(json!([
            {"duration": 1, "source_url": format!("https://shop.example.com/{}", "a".repeat(5_000))}
        ]));

        assert_eq!(
            row["long_task.top_script.source_url"]
                .as_str()
                .unwrap()
                .len(),
            2048
        );
    }

    #[test]
    fn long_text_is_cut_to_the_length_of_its_column() {
        let row = flatten(
            RumStream::Errors,
            object(
                json!({"error": {"id": "e", "message": "m".repeat(20_000), "source": "source"}}),
            ),
        );

        assert_eq!(row["error.message"].as_str().unwrap().len(), 8192);
    }

    #[test]
    fn no_column_is_also_the_parent_of_another() {
        for stream in RumStream::ALL {
            let names = fields(stream)
                .iter()
                .map(|field| field.name)
                .collect::<Vec<_>>();

            for name in &names {
                let prefix = format!("{name}.");

                assert!(
                    !names.iter().any(|other| other.starts_with(&prefix)),
                    "{name} in {stream:?} is a leaf and a container"
                );
            }
        }
    }
}
