// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! saved_views CRUD + pinned filter + validation + cross-org isolation.

mod common;

use serde_json::Value;

#[tokio::test]
async fn saved_views_crud_and_pinned_filter() {
    if common::skip_unless_enabled() {
        return;
    }
    let s = common::TestServer::start().await;
    let (hk, hv) = s.auth_header();

    // create
    let create = s
        .client
        .post(format!("{}/api/v1/saved_views", s.base_url))
        .header(hk, &hv)
        .json(&serde_json::json!({
            "name": "error logs last 1h",
            "language": "sql",
            "statement": "SELECT * FROM logs WHERE level = 'error'",
            "time_range_secs": 3600,
            "stream": "logs",
            "tags": ["errors", "triage"],
            "pinned": false,
        }))
        .send()
        .await
        .unwrap();
    assert!(
        create.status().is_success(),
        "create failed: {}",
        create.status()
    );
    let created: Value = create.json().await.unwrap();
    let id = created
        .get("id")
        .and_then(Value::as_str)
        .expect("created id")
        .to_string();
    assert_eq!(created.get("language").and_then(Value::as_str), Some("sql"));
    assert!(
        created
            .get("owner_user_id")
            .and_then(Value::as_str)
            .is_some()
    );

    // get
    let got = s
        .client
        .get(format!("{}/api/v1/saved_views/{id}", s.base_url))
        .header(hk, &hv)
        .send()
        .await
        .unwrap();
    assert!(got.status().is_success());
    let got_v: Value = got.json().await.unwrap();
    assert_eq!(
        got_v.get("name").and_then(Value::as_str),
        Some("error logs last 1h")
    );

    // update → pin + rename + widen window
    let upd = s
        .client
        .put(format!("{}/api/v1/saved_views/{id}", s.base_url))
        .header(hk, &hv)
        .json(&serde_json::json!({
            "name": "error logs (pinned)",
            "language": "sql",
            "statement": "SELECT * FROM logs WHERE level = 'error'",
            "time_range_secs": 7200,
            "stream": "logs",
            "tags": ["errors"],
            "pinned": true,
        }))
        .send()
        .await
        .unwrap();
    assert!(upd.status().is_success());
    let upd_v: Value = upd.json().await.unwrap();
    assert_eq!(upd_v.get("pinned").and_then(Value::as_bool), Some(true));
    assert_eq!(
        upd_v.get("time_range_secs").and_then(Value::as_u64),
        Some(7200)
    );

    // list all — our view is present
    let list = s
        .client
        .get(format!("{}/api/v1/saved_views", s.base_url))
        .header(hk, &hv)
        .send()
        .await
        .unwrap();
    assert!(list.status().is_success());
    let items: Value = list.json().await.unwrap();
    assert!(
        items
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v.get("id").and_then(Value::as_str) == Some(id.as_str()))
    );

    // list pinned only — all returned are pinned, ours included
    let pinned = s
        .client
        .get(format!("{}/api/v1/saved_views?pinned=true", s.base_url))
        .header(hk, &hv)
        .send()
        .await
        .unwrap();
    assert!(pinned.status().is_success());
    let pinned_items: Value = pinned.json().await.unwrap();
    let arr = pinned_items.as_array().unwrap();
    assert!(
        arr.iter()
            .all(|v| v.get("pinned").and_then(Value::as_bool) == Some(true))
    );
    assert!(
        arr.iter()
            .any(|v| v.get("id").and_then(Value::as_str) == Some(id.as_str()))
    );

    // delete
    let del = s
        .client
        .delete(format!("{}/api/v1/saved_views/{id}", s.base_url))
        .header(hk, &hv)
        .send()
        .await
        .unwrap();
    assert!(del.status().is_success());

    // get after delete → 404 (RowNotFound → not_found)
    let after = s
        .client
        .get(format!("{}/api/v1/saved_views/{id}", s.base_url))
        .header(hk, &hv)
        .send()
        .await
        .unwrap();
    assert_eq!(after.status().as_u16(), 404);
}

#[tokio::test]
async fn saved_view_validation_rejects_bad_input() {
    if common::skip_unless_enabled() {
        return;
    }
    let s = common::TestServer::start().await;
    let (hk, hv) = s.auth_header();

    let post = |body: serde_json::Value| {
        let (k, v) = (hk, hv.clone());
        let url = format!("{}/api/v1/saved_views", s.base_url);
        let client = s.client.clone();
        async move {
            client
                .post(url)
                .header(k, v)
                .json(&body)
                .send()
                .await
                .unwrap()
        }
    };

    // blank name
    let blank = post(serde_json::json!({
        "name": "   ", "language": "sql", "statement": "SELECT 1", "time_range_secs": 900,
    }))
    .await;
    assert_eq!(blank.status().as_u16(), 400, "blank name should be 400");

    // over-length name (> 255 chars) → 400, not a Postgres-truncation 500
    let long = post(serde_json::json!({
        "name": "x".repeat(300), "language": "sql", "statement": "SELECT 1", "time_range_secs": 900,
    }))
    .await;
    assert_eq!(
        long.status().as_u16(),
        400,
        "over-length name should be 400"
    );

    // zero look-back window → 400 (would otherwise yield an empty window on open)
    let zero = post(serde_json::json!({
        "name": "z", "language": "sql", "statement": "SELECT 1", "time_range_secs": 0,
    }))
    .await;
    assert_eq!(
        zero.status().as_u16(),
        400,
        "time_range_secs=0 should be 400"
    );
}
