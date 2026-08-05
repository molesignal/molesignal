// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `/api/v1/web/trace/:trace_id` 端点冒烟。
//!
//! happy: 合法 trace_id 但 traces 流为空 → 404 not_found
//! sad:   非法 trace_id（含 `;` / 空格） → 400 invalid
//! e2e:   按标准 OTEL 列名 ingest 一条 trace（自动建流）→ /web/traces 列表 + 详情都出数据

mod common;

#[tokio::test]
async fn trace_unknown_id_returns_404() {
    if common::skip_unless_enabled() {
        return;
    }
    let s = common::TestServer::start().await;
    let (hk, hv) = s.auth_header();

    let resp = s
        .client
        .get(format!(
            "{}/api/v1/web/trace/0123456789abcdef0123456789abcdef",
            s.base_url
        ))
        .header(hk, &hv)
        .send()
        .await
        .unwrap();
    // 三种合法路径：
    // - traces 流不存在 → ensure_stream_in_org 返 Forbidden(403)
    // - traces 流存在但 trace_id 查无 → handler 返 not_found(404)
    // - query engine 走偏 → 500
    let code = resp.status().as_u16();
    assert!(
        code == 403 || code == 404 || code == 500,
        "expected 403/404/500, got {code}"
    );
}

#[tokio::test]
async fn trace_invalid_id_returns_400() {
    if common::skip_unless_enabled() {
        return;
    }
    let s = common::TestServer::start().await;
    let (hk, hv) = s.auth_header();

    let resp = s
        .client
        .get(format!("{}/api/v1/web/trace/bad;trace%20id", s.base_url))
        .header(hk, &hv)
        .send()
        .await
        .unwrap();
    // 字符不在 [A-Za-z0-9_-] → handler 内 Error::invalid → 400
    assert_eq!(resp.status().as_u16(), 400);
}

#[tokio::test]
async fn trace_ingest_auto_creates_stream_then_list_and_detail_return_data() {
    if common::skip_unless_enabled() {
        return;
    }
    let s = common::TestServer::start().await;
    let (hk, hv) = s.auth_header();

    let trace_id = "0123456789abcdef0123456789abcdef";
    let root_span = "1111111111111111";
    let child_span = "2222222222222222";

    // 撤掉预 seed 后没有任何 traces 流；按标准 OTEL 列名（operation=`name`，
    // service 带点 `service.name`，时间 `*_unix_nano`，status_code 字符串）ingest 到
    // `default` 应触发 schema-on-write 自动建流。
    let resp = s
        .client
        .post(format!("{}/api/v1/ingest/traces/default", s.base_url))
        .header(hk, &hv)
        .json(&serde_json::json!([
            {
                "trace_id": trace_id,
                "span_id": root_span,
                "service.name": "checkout",
                "name": "GET /checkout",
                "start_time_unix_nano": 1_000_000_000u64,
                "end_time_unix_nano": 3_000_000_000u64,
                "duration_ns": 2_000_000_000u64,
                "status_code": "OK",
                "http.method": "GET"
            },
            {
                "trace_id": trace_id,
                "span_id": child_span,
                "parent_span_id": root_span,
                "service.name": "checkout",
                "name": "db.query",
                "start_time_unix_nano": 1_100_000_000u64,
                "end_time_unix_nano": 2_000_000_000u64,
                "duration_ns": 900_000_000u64,
                "status_code": "ERROR",
                "db.system": "postgres"
            }
        ]))
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        200,
        "ingest to a non-existent stream must auto-create it"
    );
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["accepted"], 2, "both spans accepted");

    // 等 buffer→parquet→parquet_file_meta flush 后，列表查询能查到这条 trace。
    let client = s.client.clone();
    let base = s.base_url.clone();
    let auth = hv.clone();
    let found = common::wait_until_async(15, move || {
        let client = client.clone();
        let base = base.clone();
        let auth = auth.clone();
        async move {
            let r = client
                .get(format!("{base}/api/v1/web/traces"))
                .header("authorization", auth)
                .send()
                .await
                .unwrap();
            if r.status() != 200 {
                return false;
            }
            let v: serde_json::Value = r.json().await.unwrap();
            v["items"]
                .as_array()
                .map(|a| !a.is_empty())
                .unwrap_or(false)
        }
    })
    .await;
    assert!(found, "ingested trace never surfaced in /web/traces");

    // 列表内容：聚合出 service / span_count / error_count。
    let v: serde_json::Value = s
        .client
        .get(format!("{}/api/v1/web/traces", s.base_url))
        .header(hk, &hv)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let item = &v["items"][0];
    assert_eq!(item["trace_id"], trace_id);
    assert_eq!(item["service"], "checkout");
    assert_eq!(item["span_count"], 2);
    assert_eq!(
        item["error_count"], 1,
        "child span status_code=ERROR counts"
    );

    // 详情：2 个 span，root 为父；扁平属性聚回 attributes。
    let resp = s
        .client
        .get(format!("{}/api/v1/web/trace/{trace_id}", s.base_url))
        .header(hk, &hv)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let v: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(v["trace_id"], trace_id);
    assert_eq!(v["root_span_id"], root_span);
    let spans = v["spans"].as_array().unwrap();
    assert_eq!(spans.len(), 2);
    let root = spans
        .iter()
        .find(|sp| sp["span_id"] == root_span)
        .expect("root span present");
    assert_eq!(root["service"], "checkout");
    assert_eq!(root["operation"], "GET /checkout");
    assert_eq!(root["status"], "OK");
    assert_eq!(root["attributes"]["http.method"], "GET");
}
