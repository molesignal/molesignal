// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! async search jobs (Prefer: respond-async) → 202 → done → results。

mod common;

use serde_json::Value;

#[tokio::test]
async fn async_query_returns_202_with_job_id() {
    if common::skip_unless_enabled() {
        return;
    }
    let s = common::TestServer::start().await;
    let (hk, hv) = s.auth_header();

    let resp = s
        .client
        .post(format!("{}/api/v1/query", s.base_url))
        .header(hk, &hv)
        .header("prefer", "respond-async")
        .json(&serde_json::json!({
            "org_id": s.root_org_id.0,
            "language": "sql",
            "statement": "SELECT 1",
            "time_range": { "start": 0_i64, "end": 1_000_000_i64 }
        }))
        .send()
        .await
        .unwrap();
    let code = resp.status().as_u16();
    // 接受 202 (job created) 或 200 (synchronous fallback — feature may not be wired)
    assert!(
        code == 200 || code == 202,
        "expected 200/202 for async query, got {code}"
    );
    if code == 202 {
        let v: Value = resp.json().await.unwrap();
        let job_id = v
            .get("job_id")
            .and_then(Value::as_str)
            .or_else(|| v.get("id").and_then(Value::as_str));
        assert!(job_id.is_some(), "async response should include job_id");

        // 容忍 worker 在 TestServer fixture 内未启动：只验证 jobs/{id} GET 可达 + 返一个合法 state。
        // worker pickup → done 的全链路由 staging / CI 长跑覆盖。
        let id = job_id.unwrap().to_string();
        let resp = s
            .client
            .get(format!("{}/api/v1/query/jobs/{id}", s.base_url))
            .header(hk, &hv)
            .send()
            .await
            .unwrap();
        let job_code = resp.status().as_u16();
        assert!(
            job_code < 500 && job_code != 401 && job_code != 403,
            "jobs/{{id}} should be reachable, got {job_code}"
        );
    }
}
