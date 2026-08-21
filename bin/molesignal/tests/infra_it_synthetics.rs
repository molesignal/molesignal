// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Embedded Probe execution through PostgreSQL result and health-state persistence.

mod common;

use reqwest::StatusCode;
use serde_json::{Value, json};

#[tokio::test]
async fn embedded_probe_distinguishes_test_and_operational_runs() {
    if common::skip_unless_enabled() {
        return;
    }
    let server = common::TestServer::start().await;
    let (header_name, header_value) = server.auth_header();

    let created = server
        .client
        .post(format!("{}/api/v1/synthetics/monitors", server.base_url))
        .header(header_name, &header_value)
        .json(&json!({
            "name": "Embedded persistence test",
            "description": "",
            "spec": {
                "kind": "http",
                "configuration": {
                    "steps": [{
                        "id": "health",
                        "name": "Health",
                        "method": "GET",
                        "url": { "source": "literal", "value": format!("{}/api/v1/healthz", server.base_url) },
                        "headers": [],
                        "query": [],
                        "body": null,
                        "extractions": [],
                        "assertions": []
                    }],
                    "follow_redirects": true,
                    "max_redirects": 5,
                    "verify_tls": true
                }
            },
            "schedule": { "kind": "interval", "every_seconds": 60 },
            "timeout_millis": 5_000,
            "max_retries": 0,
            "consecutive_failures": 1,
            "consecutive_recoveries": 2,
            "freshness_seconds": 60,
            "location_policy": { "kind": "any" },
            "location_ids": ["builtin-local"],
            "team_id": null,
            "tags": [],
            "escalation_policy_id": null,
            "alert_on_degraded": false,
            "alert_on_flaky": false
        }))
        .send()
        .await
        .expect("create synthetic monitor");
    assert_eq!(created.status(), StatusCode::CREATED);
    let created: Value = created.json().await.expect("created monitor body");
    let monitor_id = created["monitor"]["id"].as_str().expect("monitor id");
    let revision_id = created["revision"]["id"].as_str().expect("revision id");

    let queued = server
        .client
        .post(format!(
            "{}/api/v1/synthetics/monitors/{monitor_id}/revisions/{revision_id}/test",
            server.base_url
        ))
        .header(header_name, &header_value)
        .send()
        .await
        .expect("queue synthetic test run");
    assert_eq!(queued.status(), StatusCode::ACCEPTED);

    let mut persisted = None;
    for _ in 0..100 {
        let response = server
            .client
            .get(format!(
                "{}/api/v1/synthetics/monitors/{monitor_id}/results?limit=10",
                server.base_url
            ))
            .header(header_name, &header_value)
            .send()
            .await
            .expect("list synthetic results");
        assert_eq!(response.status(), StatusCode::OK);
        let results: Vec<Value> = response.json().await.expect("synthetic result list");
        if let Some(result) = results.into_iter().next() {
            persisted = Some(result);
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    let result = persisted.expect("embedded Probe result persisted within ten seconds");
    assert_eq!(result["monitor_revision_id"], revision_id);
    assert_eq!(result["is_test"], true);
    assert_eq!(result["outcome"], "healthy");

    let mut revision_tested = false;
    for _ in 0..100 {
        let response = server
            .client
            .get(format!(
                "{}/api/v1/synthetics/monitors/{monitor_id}",
                server.base_url
            ))
            .header(header_name, &header_value)
            .send()
            .await
            .expect("get synthetic monitor after test run");
        assert_eq!(response.status(), StatusCode::OK);
        let detail: Value = response.json().await.expect("synthetic monitor detail");
        revision_tested = detail["revisions"]
            .as_array()
            .and_then(|revisions| {
                revisions
                    .iter()
                    .find(|revision| revision["id"] == revision_id)
            })
            .is_some_and(|revision| !revision["last_test_passed_at"].is_null());
        if revision_tested {
            assert_eq!(detail["monitor"]["state"], "unknown");
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert!(revision_tested, "test result marks the revision as tested");

    let published = server
        .client
        .post(format!(
            "{}/api/v1/synthetics/monitors/{monitor_id}/revisions/{revision_id}/publish",
            server.base_url
        ))
        .header(header_name, &header_value)
        .send()
        .await
        .expect("publish tested synthetic revision");
    assert_eq!(published.status(), StatusCode::OK);

    let run = server
        .client
        .post(format!(
            "{}/api/v1/synthetics/monitors/{monitor_id}/run",
            server.base_url
        ))
        .header(header_name, &header_value)
        .send()
        .await
        .expect("queue operational synthetic run");
    assert_eq!(run.status(), StatusCode::ACCEPTED);
    let tasks: Vec<Value> = run.json().await.expect("operational Probe tasks");
    assert!(!tasks.is_empty());
    assert!(tasks.iter().all(|task| task["is_test"] == false));

    let mut operational_result = None;
    for _ in 0..100 {
        let response = server
            .client
            .get(format!(
                "{}/api/v1/synthetics/monitors/{monitor_id}/results?limit=10",
                server.base_url
            ))
            .header(header_name, &header_value)
            .send()
            .await
            .expect("list synthetic results after operational run");
        assert_eq!(response.status(), StatusCode::OK);
        let results: Vec<Value> = response.json().await.expect("synthetic result list");
        if let Some(result) = results
            .into_iter()
            .find(|result| result["is_test"] == false)
        {
            operational_result = Some(result);
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    let operational_result =
        operational_result.expect("operational Probe result persisted within ten seconds");
    assert_eq!(operational_result["outcome"], "healthy");

    let mut monitor_state = None;
    for _ in 0..100 {
        let response = server
            .client
            .get(format!(
                "{}/api/v1/synthetics/monitors/{monitor_id}",
                server.base_url
            ))
            .header(header_name, &header_value)
            .send()
            .await
            .expect("get synthetic monitor after operational run");
        assert_eq!(response.status(), StatusCode::OK);
        let detail: Value = response.json().await.expect("synthetic monitor detail");
        monitor_state = detail["monitor"]["state"].as_str().map(str::to_owned);
        if monitor_state.as_deref() == Some("healthy") {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert_eq!(monitor_state.as_deref(), Some("healthy"));

    let agents = server
        .client
        .get(format!("{}/api/v1/synthetics/agents", server.base_url))
        .header(header_name, &header_value)
        .send()
        .await
        .expect("list synthetic agents");
    assert_eq!(agents.status(), StatusCode::OK);
    let agents: Vec<Value> = agents.json().await.expect("synthetic agent inventory");
    assert!(
        agents
            .iter()
            .any(|agent| agent["location_id"] == "builtin-local"),
        "organization Agent inventory includes the embedded platform Probe"
    );

    let configured = server
        .client
        .put(format!(
            "{}/api/v1/synthetics/agents/builtin-local-runner/configuration",
            server.base_url
        ))
        .header(header_name, &header_value)
        .json(&json!({
            "name": "Local Probe · integration",
            "labels": {
                "environment": "test",
                "system_managed": "false"
            }
        }))
        .send()
        .await
        .expect("update organization Agent configuration");
    assert_eq!(configured.status(), StatusCode::OK);
    let configured: Value = configured.json().await.expect("configured Agent body");
    assert_eq!(configured["name"], "Local Probe · integration");
    assert_eq!(configured["labels"]["environment"], "test");
    assert_eq!(configured["labels"]["system_managed"], "true");

    let agents = server
        .client
        .get(format!("{}/api/v1/synthetics/agents", server.base_url))
        .header(header_name, &header_value)
        .send()
        .await
        .expect("list configured synthetic agents");
    assert_eq!(agents.status(), StatusCode::OK);
    let agents: Vec<Value> = agents.json().await.expect("configured Agent inventory");
    let embedded = agents
        .iter()
        .find(|agent| agent["id"] == "builtin-local-runner")
        .expect("configured embedded Probe remains visible");
    assert_eq!(embedded["name"], "Local Probe · integration");
    assert_eq!(embedded["labels"]["environment"], "test");
}
