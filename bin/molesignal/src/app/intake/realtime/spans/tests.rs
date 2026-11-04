// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde_json::json;

use super::*;

fn fields(value: Value) -> Map<String, Value> {
    value.as_object().unwrap().clone()
}

#[test]
fn the_attribute_json_follows_the_flat_attributes() {
    let before = fields(json!({
        "name": "GET /users/42",
        "http.url": "https://shop.example/users/42?token=abc",
        "db.statement": "SELECT 1",
        "host.name": "web-01",
        "attributes": {
            "http.url": "https://shop.example/users/42?token=abc",
            "db.statement": "SELECT 1",
            "peer": "db",
        },
        "resource": { "attributes": { "host.name": "web-01", "service.name": "shop" } },
    }));

    let mut after = before.clone();
    after.insert("http.url".into(), json!("https://shop.example/users/{id}"));
    after.remove("db.statement");
    after.remove("host.name");
    after.insert("team".into(), json!("payments"));
    after.insert("status_message".into(), json!("ok"));
    after["attributes"]["peer"] = json!("database");

    follow_attributes(&before, &mut after);

    assert_eq!(
        after["attributes"],
        json!({
            "http.url": "https://shop.example/users/{id}",
            "peer": "database",
            "team": "payments",
        })
    );
    assert_eq!(
        after["resource"],
        json!({ "attributes": { "service.name": "shop" } })
    );
}

#[test]
fn a_span_without_attribute_json_is_left_as_the_steps_left_it() {
    let before = fields(json!({ "name": "a", "db.statement": "SELECT 1" }));
    let mut after = fields(json!({ "name": "a", "team": "payments" }));

    follow_attributes(&before, &mut after);

    assert_eq!(
        Value::Object(after),
        json!({ "name": "a", "team": "payments" })
    );
}
