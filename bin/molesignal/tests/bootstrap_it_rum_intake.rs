// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! RUM intake 冒烟：先建应用，再按事件契约写事件和回放片段。
//!
//! happy path：应用 → `POST /rum/events` 全部接收 → 读回应用列表；
//! sad path：未知应用的事件被单条拒绝，体不是 JSON → 4xx，旧的分类型写入口不存在。

mod common;

use serde_json::{Value, json};

async fn create_application(s: &common::TestServer, id: &str) {
    let (hk, hv) = s.auth_header();

    let resp = s
        .client
        .post(format!("{}/api/v1/rum/applications", s.base_url))
        .header(hk, &hv)
        .json(&json!({ "name": "Test mobile", "id": id, "platform": "flutter" }))
        .send()
        .await
        .unwrap();

    let code = resp.status().as_u16();

    assert!(
        (200..300).contains(&code),
        "application create failed: {code}"
    );
}

fn view(application: &str) -> Value {
    json!({
        "type": "view",
        "date": 1_700_000_000_000_i64,
        "application": { "id": application },
        "service": "mobile",
        "env": "prod",
        "version": "1.0.0",
        "source": "flutter",
        "session": { "id": "s-1", "type": "user" },
        "view": { "id": "v-1", "url": "app://home", "name": "home", "revision": 1, "time_spent": 5_000_000_000_i64 },
    })
}

fn resource(application: &str) -> Value {
    json!({
        "type": "resource",
        "date": 1_700_000_001_000_i64,
        "application": { "id": application },
        "service": "mobile",
        "env": "prod",
        "version": "1.0.0",
        "source": "flutter",
        "session": { "id": "s-1", "type": "user" },
        "view": { "id": "v-1" },
        "resource": {
            "id": "r-1", "type": "fetch", "method": "GET", "url": "https://api.example.com/cart",
            "status_code": 200, "duration": 120_000_000_i64,
            "trace_id": "4bf92f3577b34da6a3ce929d0e0e4736", "span_id": "00f067aa0ba902b7",
        },
    })
}

#[tokio::test]
async fn rum_events_endpoint_accepts_valid_events() {
    if common::skip_unless_enabled() {
        return;
    }
    let s = common::TestServer::start().await;
    let (hk, hv) = s.auth_header();
    create_application(&s, "test-mobile").await;

    let resp = s
        .client
        .post(format!("{}/api/v1/rum/events", s.base_url))
        .header(hk, &hv)
        .json(&json!([view("test-mobile"), resource("test-mobile")]))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status().as_u16(), 200);

    let report: Value = resp.json().await.unwrap();

    assert_eq!(report["accepted"], 2, "{report}");
    assert_eq!(report["rejected"], json!([]), "{report}");
}

#[tokio::test]
async fn rum_events_of_an_unknown_application_are_refused_one_by_one() {
    if common::skip_unless_enabled() {
        return;
    }
    let s = common::TestServer::start().await;
    let (hk, hv) = s.auth_header();
    create_application(&s, "test-mobile").await;

    let resp = s
        .client
        .post(format!("{}/api/v1/rum/events", s.base_url))
        .header(hk, &hv)
        .json(&json!([view("test-mobile"), view("ghost")]))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status().as_u16(), 200);

    let report: Value = resp.json().await.unwrap();

    assert_eq!(report["accepted"], 1, "{report}");
    assert_eq!(report["rejected"][0]["index"], 1, "{report}");
    assert!(
        report["rejected"][0]["reason"]
            .as_str()
            .is_some_and(|reason| reason.contains("ghost")),
        "{report}"
    );
}

#[tokio::test]
async fn rum_events_reject_a_body_that_is_not_json() {
    if common::skip_unless_enabled() {
        return;
    }
    let s = common::TestServer::start().await;
    let (hk, hv) = s.auth_header();

    let resp = s
        .client
        .post(format!("{}/api/v1/rum/events", s.base_url))
        .header(hk, &hv)
        .body("not-json")
        .send()
        .await
        .unwrap();
    let code = resp.status().as_u16();

    assert!(
        code == 400 || code == 415 || code == 422,
        "bad payload should be 4xx, got {code}"
    );
}

#[tokio::test]
async fn rum_applications_list_what_was_created() {
    if common::skip_unless_enabled() {
        return;
    }
    let s = common::TestServer::start().await;
    let (hk, hv) = s.auth_header();
    create_application(&s, "test-mobile").await;

    let resp = s
        .client
        .get(format!("{}/api/v1/rum/applications", s.base_url))
        .header(hk, &hv)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status().as_u16(), 200);

    let applications: Value = resp.json().await.unwrap();

    assert_eq!(applications[0]["id"], "test-mobile", "{applications}");
    assert_eq!(applications[0]["platform"], "flutter", "{applications}");
}

#[tokio::test]
async fn rum_replay_endpoint_accepts_json_segment() {
    if common::skip_unless_enabled() {
        return;
    }
    let s = common::TestServer::start().await;
    let (hk, hv) = s.auth_header();
    create_application(&s, "test-mobile").await;

    let body = json!({
        "application": "test-mobile",
        "sessionId": "s-1",
        "seq": 1,
        "events": [{
            "type": 2,
            "timestamp": 1_700_000_000_000_u64,
            "data": {"node": {"type": 0}}
        }]
    });
    let resp = s
        .client
        .post(format!("{}/api/v1/rum/replay", s.base_url))
        .header(hk, &hv)
        .json(&body)
        .send()
        .await
        .unwrap();
    let code = resp.status().as_u16();
    assert!(
        (200..300).contains(&code),
        "RUM replay intake failed: {code}"
    );
}

#[tokio::test]
async fn the_write_paths_of_each_kind_of_event_are_not_registered() {
    if common::skip_unless_enabled() {
        return;
    }
    let s = common::TestServer::start().await;
    let (hk, hv) = s.auth_header();

    for path in ["sessions", "actions", "errors", "v1/errors"] {
        let resp = s
            .client
            .post(format!("{}/api/v1/rum/{path}", s.base_url))
            .header(hk, &hv)
            .json(&json!([]))
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status().as_u16(), 404, "{path}");
    }
}
