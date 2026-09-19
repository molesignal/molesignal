// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Metric catalog HTTP route regression coverage.

mod common;

#[tokio::test]
async fn metric_catalog_is_mounted_under_api_v1() {
    if common::skip_unless_enabled() {
        return;
    }
    let server = common::TestServer::start().await;
    let (header_name, header_value) = server.auth_header();

    let response = server
        .client
        .get(format!("{}/api/v1/metrics/catalog", server.base_url))
        .header(header_name, header_value)
        .send()
        .await
        .expect("request metric catalog");

    assert_eq!(
        response.status(),
        reqwest::StatusCode::OK,
        "metric catalog must be mounted under /api/v1"
    );
    let body: serde_json::Value = response.json().await.expect("decode metric catalog");
    assert!(body["metrics"].is_array(), "catalog response: {body}");
}
