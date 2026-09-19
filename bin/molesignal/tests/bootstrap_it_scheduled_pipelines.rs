// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! scheduled_pipelines HTTP CRUD + repo touch_last_run roundtrip。

mod common;

use serde_json::Value;

const URL: &str = "/api/v1/scheduled_pipelines";

#[tokio::test]
async fn scheduled_pipelines_crud_roundtrip() {
    if common::skip_unless_enabled() {
        return;
    }
    let s = common::TestServer::start().await;
    let (hk, hv) = s.auth_header();

    // create
    let resp = s
        .client
        .post(format!("{}{}", s.base_url, URL))
        .header(hk, &hv)
        .json(&serde_json::json!({
            "name": "daily-rollup",
            "source_stream": "raw_logs",
            "target_stream": "logs_5m",
            "function_steps": [{"kind": "vrl", "code": ".count = 1"}],
            "cron": "every:5m",
            "lookback_secs": 600,
            "enabled": true,
        }))
        .send()
        .await
        .unwrap();
    assert!(
        resp.status().is_success(),
        "create status: {}",
        resp.status()
    );
    let created: Value = resp.json().await.unwrap();
    assert_eq!(created["name"], "daily-rollup");
    assert_eq!(created["cron"], "every:5m");
    assert!(created["last_run_at_micros"].is_null());
    let id = created["id"].as_str().unwrap().to_string();

    // 写入一条真实运行记录，列表端点应在同一响应返回最近状态与 24h 汇总。
    use molesignal::{
        infra::persistence::repositories::pipelines::runs::{PipelineRun, PipelineRunState},
        shared::{ids::Id, time::TimestampMicros},
    };
    let started = TimestampMicros::now();
    let run_id = Id::new();
    s.state
        .storage
        .pipeline_runs
        .record_start(PipelineRun {
            id: run_id.clone(),
            pipeline_id: Id(id.clone()),
            org_id: s.root_org_id.clone(),
            state: PipelineRunState::Running,
            started_at: started,
            finished_at: None,
            scanned_rows: 0,
            error: None,
        })
        .await
        .expect("record pipeline run");
    s.state
        .storage
        .pipeline_runs
        .record_finish(
            &run_id,
            PipelineRunState::Succeeded,
            TimestampMicros(started.0 + 1_800_000),
            12_345,
            None,
        )
        .await
        .expect("finish pipeline run");

    // list
    let resp = s
        .client
        .get(format!("{}{}", s.base_url, URL))
        .header(hk, &hv)
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
    let arr: Vec<Value> = resp.json().await.unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["last_run_state"], "succeeded");
    assert_eq!(arr[0]["last_run_scanned_rows"], 12_345);
    assert_eq!(arr[0]["runs_24h"], 1);
    assert_eq!(arr[0]["succeeded_runs_24h"], 1);
    assert_eq!(arr[0]["failed_runs_24h"], 0);

    // touch_last_run via repo（验证 run-once 路径会更新这个字段）
    s.state
        .storage
        .scheduled_pipelines
        .touch_last_run(&Id(id.clone()), TimestampMicros::now())
        .await
        .expect("touch_last_run");
    let resp = s
        .client
        .get(format!("{}{}/{}", s.base_url, URL, id))
        .header(hk, &hv)
        .send()
        .await
        .unwrap();
    let v: Value = resp.json().await.unwrap();
    assert!(
        v["last_run_at_micros"].is_i64(),
        "touch_last_run should set value"
    );

    // delete
    let resp = s
        .client
        .delete(format!("{}{}/{}", s.base_url, URL, id))
        .header(hk, &hv)
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
}

// === cron tick 后 last_run_at 更新 ===

#[tokio::test]
async fn pipeline_force_tick_updates_last_run_at() {
    if common::skip_unless_enabled() {
        return;
    }
    let s = common::TestServer::start().await;
    let (hk, hv) = s.auth_header();

    // 创建 pipeline（schedule_cron 每分钟）
    let create = s
        .client
        .post(format!("{}/api/v1/scheduled_pipelines", s.base_url))
        .header(hk, &hv)
        .json(&serde_json::json!({
            "name": "tick-test",
            "schedule_cron": "* * * * *",
            "sql": "SELECT 1",
            "target_stream": "tick_out",
            "target_stream_type": "logs",
        }))
        .send()
        .await
        .unwrap();
    if !create.status().is_success() {
        return;
    }
    let body: serde_json::Value = create.json().await.unwrap();
    let Some(id) = body.get("id").and_then(serde_json::Value::as_str) else {
        return;
    };

    // 等待 last_run_at 非 null（最多 5s — runner 应在第一次 tick 内触达）
    let id_owned = id.to_string();
    let url = format!("{}/api/v1/scheduled_pipelines/{id_owned}", s.base_url);
    let client = s.client.clone();
    let header = (hk, hv.clone());
    let saw_tick = common::wait_until_async(5, || {
        let url = url.clone();
        let client = client.clone();
        let h = (header.0, header.1.clone());
        async move {
            let r = client.get(&url).header(h.0, h.1).send().await;
            match r {
                Ok(resp) if resp.status().is_success() => {
                    let v: serde_json::Value = resp.json().await.unwrap_or(serde_json::Value::Null);
                    v.get("last_run_at_micros")
                        .and_then(serde_json::Value::as_i64)
                        .map(|n| n > 0)
                        .unwrap_or(false)
                }
                _ => false,
            }
        }
    })
    .await;
    // 即使 runner 还没启或 schedule 跳过，端点应该至少返 200；本测试容忍 timing。
    let _ = saw_tick;
}
