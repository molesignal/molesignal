// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

mod common;
use serde_json::{Value, json};

#[tokio::test]
async fn realtime_configuration_is_unique_and_cannot_be_backfilled() {
    if common::skip_unless_enabled() {
        return;
    }
    let s = common::TestServer::start().await;
    let (hk, hv) = s.auth_header();
    let url = format!("{}/api/v1/scheduled_pipelines", s.base_url);
    let payload = json!({"name":"app routing","source_stream":"default","target_stream":"default","cron":"","lookback_secs":300,"enabled":true,
        "function_steps":{"mode":"realtime","signal_type":"logs","sources":["default"],"sinks":["default"],"steps":[],"routing":{"kind":"field","field":"appname","fallback":"default","retain_source":true}}});
    let response = s
        .client
        .post(&url)
        .header(hk, &hv)
        .json(&payload)
        .send()
        .await
        .unwrap();
    assert!(
        response.status().is_success(),
        "{}",
        response.text().await.unwrap()
    );
    let created: Value = response.json().await.unwrap();
    let id = created["id"].as_str().unwrap();
    let duplicate = s
        .client
        .post(&url)
        .header(hk, &hv)
        .json(&payload)
        .send()
        .await
        .unwrap();
    assert_eq!(duplicate.status().as_u16(), 409);
    let backfill = s
        .client
        .post(format!("{url}/{id}/backfill"))
        .header(hk, &hv)
        .json(&json!({"start_micros":1,"end_micros":2}))
        .send()
        .await
        .unwrap();
    assert_eq!(backfill.status().as_u16(), 400);
    let repo = molesignal::infra::pipeline::realtime::PgRealtimePipelineRepository::new(
        sqlx::postgres::PgPoolOptions::new()
            .max_connections(2)
            .connect(&s.settings.store.meta.dsn)
            .await
            .unwrap(),
    );
    use molesignal::{
        domain::{
            intake::{IntakeBatch, RawEvent},
            stream::StreamType,
        },
        shared::{ids::Id, time::TimestampMicros},
    };
    let now = TimestampMicros::now();
    let result = s
        .state
        .intake
        .intake(IntakeBatch {
            batch_id: Id::new(),
            org_id: s.root_org_id.clone(),
            stream: "default".into(),
            stream_type: StreamType::LOGS,
            events: vec![RawEvent {
                timestamp: now,
                fields: json!({"appname":"realtime_orders","message":"integration"})
                    .as_object()
                    .unwrap()
                    .clone(),
            }],
            received_at: now,
        })
        .await
        .unwrap();
    assert_eq!((result.accepted, result.rejected), (1, 0));
    assert!(
        s.state
            .telemetry
            .streams
            .get(&s.root_org_id, "realtime_orders", StreamType::LOGS)
            .await
            .is_ok()
    );
    assert!(
        s.state
            .telemetry
            .streams
            .get(&s.root_org_id, "default", StreamType::LOGS)
            .await
            .is_ok()
    );
    // Repository projection must keep the organization predicate.
    use molesignal::domain::pipeline::realtime::RealtimePipelineRepository;
    assert!(
        repo.for_source(&s.root_org_id, "default")
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        repo.for_source(&molesignal::shared::ids::Id::new(), "default")
            .await
            .unwrap()
            .is_none()
    );
}
