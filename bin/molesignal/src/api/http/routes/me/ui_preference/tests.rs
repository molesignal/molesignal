// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde_json::json;

use super::*;

#[test]
fn a_purpose_is_one_this_product_knows() {
    assert_eq!(Scope::parse("logs.columns").unwrap(), Scope::LogColumns);
    assert_eq!(Scope::LogColumns.as_str(), "logs.columns");
    assert!(Scope::parse("logs").is_err());
    assert!(Scope::parse("anything.else").is_err());
}

#[test]
fn a_trace_view_keeps_its_columns_and_widths() {
    assert_eq!(
        Scope::parse("traces.columns").unwrap().as_str(),
        "traces.columns"
    );
    assert_eq!(
        Scope::parse("traces.layout").unwrap().as_str(),
        "traces.layout"
    );

    let columns = Scope::TraceColumns;
    assert!(
        columns
            .validate(&json!({ "fields": ["http.route"] }))
            .is_ok()
    );
    assert!(columns.validate(&json!({ "fields": ["a", "a"] })).is_err());
    let thirteen = (0..13).map(|index| format!("f{index}")).collect::<Vec<_>>();
    assert!(columns.validate(&json!({ "fields": thirteen })).is_err());

    let layout = Scope::TraceLayout;
    assert!(
        layout
            .validate(&json!({ "drawer": 1200, "panel": 360 }))
            .is_ok()
    );
    assert!(layout.validate(&json!({})).is_ok());
    assert!(layout.validate(&json!({ "panel": 20 })).is_err());
    assert!(layout.validate(&json!({ "height": 300 })).is_err());
}

#[test]
fn log_columns_are_field_names_in_an_order() {
    let scope = Scope::LogColumns;

    assert!(
        scope
            .validate(&json!({ "fields": ["level", "service.name", "message"] }))
            .is_ok()
    );
    // No column beside the time is an arrangement too.
    assert!(scope.validate(&json!({ "fields": [] })).is_ok());
}

#[test]
fn log_columns_are_refused_in_any_other_shape() {
    let scope = Scope::LogColumns;

    assert!(scope.validate(&json!(["level"])).is_err());
    assert!(scope.validate(&json!({ "fields": "level" })).is_err());
    assert!(
        scope
            .validate(&json!({ "fields": ["level"], "widths": {} }))
            .is_err()
    );
    assert!(scope.validate(&json!({ "fields": ["level", 3] })).is_err());
    assert!(scope.validate(&json!({ "fields": [""] })).is_err());
    assert!(
        scope
            .validate(&json!({ "fields": ["level", "level"] }))
            .is_err()
    );
    assert!(
        scope
            .validate(&json!({ "fields": ["x".repeat(256)] }))
            .is_err()
    );

    let many: Vec<String> = (0..=MAX_LOG_COLUMNS)
        .map(|index| format!("f{index}"))
        .collect();
    assert!(scope.validate(&json!({ "fields": many })).is_err());
}

#[test]
fn pinned_answers_are_ids_in_an_order() {
    let scope = Scope::parse("agent.message_pins").unwrap();
    assert_eq!(scope.as_str(), "agent.message_pins");

    assert!(
        scope
            .validate(&json!({ "messages": ["m-2", "m-1"] }))
            .is_ok()
    );
    assert!(scope.validate(&json!({ "messages": [] })).is_ok());
    assert!(
        scope
            .validate(&json!({ "messages": ["m-1", "m-1"] }))
            .is_err()
    );
    assert!(scope.validate(&json!({ "messages": [""] })).is_err());
    assert!(scope.validate(&json!(["m-1"])).is_err());

    let too_many: Vec<String> = (0..101).map(|n| format!("m{n}")).collect();
    assert!(scope.validate(&json!({ "messages": too_many })).is_err());
}

#[test]
fn apm_favorites_are_services_listed_once() {
    let scope = Scope::parse("apm.favorites").unwrap();
    let checkout = json!({ "namespace": "shop", "name": "checkout" });

    assert_eq!(scope.as_str(), "apm.favorites");
    assert!(scope.validate(&json!({ "services": [checkout] })).is_ok());
    assert!(scope.validate(&json!({ "services": [] })).is_ok());

    for refused in [
        json!([checkout, checkout]),
        json!([{ "name": "checkout" }]),
        json!(["shop/checkout"]),
    ] {
        assert!(scope.validate(&json!({ "services": refused })).is_err());
    }
}

#[test]
fn a_key_is_a_name_of_reasonable_length() {
    assert!(validate_key("application_logs").is_ok());
    assert!(validate_key("k8s/events.prod").is_ok());
    assert!(validate_key("").is_err());
    assert!(validate_key("   ").is_err());
    assert!(validate_key(&"k".repeat(256)).is_err());
}

#[test]
fn the_bell_keeps_a_bounded_list_of_what_was_read() {
    let scope = Scope::parse("notifications.read").unwrap();
    assert_eq!(scope.as_str(), "notifications.read");

    assert!(
        scope
            .validate(&json!({ "ids": ["incident:1", "pipeline:2:1700000000"] }))
            .is_ok()
    );
    assert!(scope.validate(&json!({ "ids": [""] })).is_err());
    assert!(
        scope
            .validate(&json!({ "ids": ["x".repeat(129)] }))
            .is_err()
    );
    assert!(scope.validate(&json!({ "ids": ["a"], "more": 1 })).is_err());

    let too_many: Vec<String> = (0..201).map(|index| index.to_string()).collect();
    assert!(scope.validate(&json!({ "ids": too_many })).is_err());
}

#[test]
fn the_views_of_the_rum_explorer_are_a_bounded_list_of_names_and_addresses() {
    let scope = Scope::parse("rum.saved_views").unwrap();
    assert_eq!(scope.as_str(), "rum.saved_views");

    let view =
        |id: &str, name: &str, search: &str| json!({ "id": id, "name": name, "search": search });

    assert!(
        scope
            .validate(&json!({ "views": [view("view-1", "Slow pages", "type=views&q=x")] }))
            .is_ok()
    );
    assert!(scope.validate(&json!({ "views": [] })).is_ok());
    assert!(
        scope
            .validate(&json!({ "views": [view("", "Slow pages", "")] }))
            .is_err()
    );
    assert!(
        scope
            .validate(&json!({ "views": [view("a", "  ", "")] }))
            .is_err()
    );
    assert!(
        scope
            .validate(&json!({ "views": [view("a", &"n".repeat(65), "")] }))
            .is_err()
    );
    assert!(
        scope
            .validate(&json!({ "views": [view("a", "n", &"s".repeat(2_049))] }))
            .is_err()
    );
    assert!(
        scope
            .validate(&json!({ "views": [view("a", "n", ""), view("a", "m", "")] }))
            .is_err()
    );

    let too_many: Vec<Value> = (0..31)
        .map(|index| view(&index.to_string(), "n", ""))
        .collect();

    assert!(scope.validate(&json!({ "views": too_many })).is_err());
}
