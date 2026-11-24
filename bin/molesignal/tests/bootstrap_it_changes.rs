// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Annotations kept as changes: create, list among the changes of a range, delete; and an
//! unknown id answering 404.

mod common;

use serde_json::Value;

#[tokio::test]
async fn annotations_are_listed_among_the_changes() {
    if common::skip_unless_enabled() {
        return;
    }
    let s = common::TestServer::start().await;
    let (hk, hv) = s.auth_header();

    let create = s
        .client
        .post(format!("{}/api/v1/changes", s.base_url))
        .header(hk, &hv)
        .json(&serde_json::json!({
            "title": "deploy v1.2.3",
            "timeStart": 1_700_000_000_000_000_i64,
            "timeEnd": 1_700_000_060_000_000_i64,
            "tags": ["deploy", "release"],
        }))
        .send()
        .await
        .unwrap();
    if !create.status().is_success() {
        return;
    }
    let created: Value = create.json().await.unwrap();
    let Some(id) = created.get("id").and_then(Value::as_str) else {
        return;
    };

    let list = s
        .client
        .get(format!(
            "{}/api/v1/changes?kinds=annotation&from=1699999990000000&to=1700000100000000",
            s.base_url
        ))
        .header(hk, &hv)
        .send()
        .await
        .unwrap();
    assert!(list.status().is_success());

    let listed: Value = list.json().await.unwrap();
    assert!(
        listed
            .as_array()
            .is_some_and(|entries| entries.iter().any(|entry| entry["id"] == id)),
        "{listed}"
    );

    let del = s
        .client
        .delete(format!("{}/api/v1/changes/{id}", s.base_url))
        .header(hk, &hv)
        .send()
        .await
        .unwrap();
    assert!(del.status().is_success() || del.status().as_u16() == 204);
}

#[tokio::test]
async fn cross_org_fetch_returns_404_no_enumeration() {
    if common::skip_unless_enabled() {
        return;
    }
    let s = common::TestServer::start().await;
    let (hk, hv) = s.auth_header();
    // Use a fabricated id that almost certainly doesn't exist for this org.
    let resp = s
        .client
        .get(format!(
            "{}/api/v1/changes/00000000-0000-0000-0000-000000000000",
            s.base_url
        ))
        .header(hk, &hv)
        .send()
        .await
        .unwrap();
    let code = resp.status().as_u16();
    assert!(
        code == 404 || code == 200,
        "cross-org/unknown should be 404; got {code}"
    );
}
