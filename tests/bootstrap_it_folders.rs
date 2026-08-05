// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 仪表盘文件夹 CRUD（`/api/v1/folders`）端到端：建 / 列 / 改名 / 三级嵌套 / 非空拒删 / 成环拒绝。

mod common;

use serde_json::{Value, json};

fn id_of(v: &Value) -> String {
    v["id"].as_str().expect("folder id").to_string()
}

#[tokio::test]
async fn folder_crud_lifecycle_and_guards() {
    if common::skip_unless_enabled() {
        return;
    }
    let s = common::TestServer::start().await;
    let (hk, hv) = s.auth_header();
    let base = &s.base_url;

    // create
    let resp = s
        .client
        .post(format!("{base}/api/v1/folders"))
        .header(hk, &hv)
        .json(&json!({ "name": "Production" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "create folder");
    let parent: Value = resp.json().await.unwrap();
    let parent_id = id_of(&parent);
    assert_eq!(parent["name"], "Production");
    assert!(parent["parent_id"].is_null());

    // empty name → 400
    let bad = s
        .client
        .post(format!("{base}/api/v1/folders"))
        .header(hk, &hv)
        .json(&json!({ "name": "   " }))
        .send()
        .await
        .unwrap();
    assert_eq!(bad.status(), 400, "empty name rejected");

    // list contains it
    let listed: Value = s
        .client
        .get(format!("{base}/api/v1/folders"))
        .header(hk, &hv)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(
        listed
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["id"] == parent["id"]),
        "list contains created folder"
    );

    // rename
    let renamed: Value = s
        .client
        .put(format!("{base}/api/v1/folders/{parent_id}"))
        .header(hk, &hv)
        .json(&json!({ "name": "Prod" }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(renamed["name"], "Prod", "rename applied");

    // nested child under parent
    let child: Value = s
        .client
        .post(format!("{base}/api/v1/folders"))
        .header(hk, &hv)
        .json(&json!({ "name": "EU", "parent_id": parent_id }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let child_id = id_of(&child);
    assert_eq!(child["parent_id"], parent_id, "child points at parent");

    // third level is supported
    let grandchild: Value = s
        .client
        .post(format!("{base}/api/v1/folders"))
        .header(hk, &hv)
        .json(&json!({ "name": "Payments", "parent_id": child_id }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let grandchild_id = id_of(&grandchild);
    assert_eq!(
        grandchild["parent_id"], child_id,
        "third-level folder points at child"
    );

    // fourth level is rejected
    let too_deep = s
        .client
        .post(format!("{base}/api/v1/folders"))
        .header(hk, &hv)
        .json(&json!({ "name": "Too deep", "parent_id": grandchild_id }))
        .send()
        .await
        .unwrap();
    assert_eq!(too_deep.status(), 400, "fourth-level folder rejected");

    // delete non-empty parent → 409
    let conflict = s
        .client
        .delete(format!("{base}/api/v1/folders/{parent_id}"))
        .header(hk, &hv)
        .send()
        .await
        .unwrap();
    assert_eq!(conflict.status(), 409, "non-empty folder delete rejected");

    // cycle: move parent under its own child → 400
    let cycle = s
        .client
        .put(format!("{base}/api/v1/folders/{parent_id}"))
        .header(hk, &hv)
        .json(&json!({ "name": "Prod", "parent_id": child_id }))
        .send()
        .await
        .unwrap();
    assert_eq!(cycle.status(), 400, "cycle move rejected");

    // delete deepest child, then its parents
    let d0 = s
        .client
        .delete(format!("{base}/api/v1/folders/{grandchild_id}"))
        .header(hk, &hv)
        .send()
        .await
        .unwrap();
    assert_eq!(d0.status(), 200, "delete empty third-level folder");
    let d1 = s
        .client
        .delete(format!("{base}/api/v1/folders/{child_id}"))
        .header(hk, &hv)
        .send()
        .await
        .unwrap();
    assert_eq!(d1.status(), 200, "delete empty child");
    let d2 = s
        .client
        .delete(format!("{base}/api/v1/folders/{parent_id}"))
        .header(hk, &hv)
        .send()
        .await
        .unwrap();
    assert_eq!(d2.status(), 200, "delete now-empty parent");

    // unknown id → 404
    let missing = s
        .client
        .put(format!("{base}/api/v1/folders/nonexistent-xyz"))
        .header(hk, &hv)
        .json(&json!({ "name": "x" }))
        .send()
        .await
        .unwrap();
    assert_eq!(missing.status(), 404, "rename of unknown folder is 404");
}
