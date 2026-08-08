// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 服务自身三信号回灌的 standalone 端到端测试。默认跳过；设置 `MS_RUN_IT=1`
//! 后使用 Postgres testcontainer 与本地 object store 执行。

mod common;

#[cfg(all(feature = "profiling-pprof", unix))]
mod enabled {
    use std::{fs, path::Path};

    use molesignal::{
        api::grpc::ingest_server::IngestGrpc,
        domain::{
            ingestion::RawEvent,
            stream::{MOLESIGNAL_SYSTEM_STREAM, StreamType},
        },
        shared::{metrics::register_int_counter, time::TimestampMicros},
    };
    use opentelemetry_proto::tonic::{
        collector::logs::v1::ExportLogsServiceRequest,
        common::v1::{AnyValue, any_value},
        logs::v1::{LogRecord, ResourceLogs, ScopeLogs},
    };
    use prost::Message;
    use serde_json::{Map, Value, json};
    use tonic::Request;

    use super::common::{TestServer, skip_unless_enabled, wait_until_async};

    #[derive(Clone, PartialEq, Message)]
    struct PromLabel {
        #[prost(string, tag = "1")]
        name: String,
        #[prost(string, tag = "2")]
        value: String,
    }

    #[derive(Clone, PartialEq, Message)]
    struct PromSample {
        #[prost(double, tag = "1")]
        value: f64,
        #[prost(int64, tag = "2")]
        timestamp: i64,
    }

    #[derive(Clone, PartialEq, Message)]
    struct PromTimeSeries {
        #[prost(message, repeated, tag = "1")]
        labels: Vec<PromLabel>,
        #[prost(message, repeated, tag = "2")]
        samples: Vec<PromSample>,
    }

    #[derive(Clone, PartialEq, Message)]
    struct PromWriteRequest {
        #[prost(message, repeated, tag = "1")]
        timeseries: Vec<PromTimeSeries>,
    }

    fn has_profile_archive(path: &Path) -> bool {
        let Ok(entries) = fs::read_dir(path) else {
            return false;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if has_profile_archive(&path) {
                    return true;
                }
            } else if path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(".pprof.zst"))
            {
                return true;
            }
        }
        false
    }

    async fn query_count(
        server: &TestServer,
        token: &str,
        org_id: &str,
        stream_type: StreamType,
    ) -> i64 {
        query_count_for_name(server, token, org_id, stream_type, MOLESIGNAL_SYSTEM_STREAM).await
    }

    async fn query_count_for_name(
        server: &TestServer,
        token: &str,
        org_id: &str,
        stream_type: StreamType,
        stream_name: &str,
    ) -> i64 {
        let stream_type = match stream_type {
            StreamType::Logs => "logs",
            StreamType::Metrics => "metrics",
            StreamType::Traces => "traces",
            StreamType::Profiles => "profiles",
            StreamType::Extend => unreachable!(),
        };
        let response = server
            .client
            .post(format!("{}/api/v1/query", server.base_url))
            .bearer_auth(token)
            .json(&json!({
                "org_id": org_id,
                "language": "sql",
                "statement": format!("SELECT COUNT(*) AS n FROM \"{stream_name}\""),
                "time_range": {
                    "start": 0,
                    "end": TimestampMicros::now().0 + 10_000_000
                },
                "stream": {
                    "name": stream_name,
                    "stream_type": stream_type
                }
            }))
            .send()
            .await
            .unwrap();
        let status = response.status();
        let body = response.text().await.unwrap();
        assert_eq!(status, 200, "query {stream_type}: {body}");
        serde_json::from_str::<Value>(&body).unwrap()["rows"][0][0]
            .as_i64()
            .unwrap_or_default()
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn all_three_self_signals_are_archived_and_queryable() {
        if skip_unless_enabled() {
            eprintln!("skipped (set MS_RUN_IT=1 to enable)");
            return;
        }
        let server = TestServer::start_with_trace_capture().await;
        let system_org_id = server.state.iam.system_org_id.clone();
        server
            .state
            .iam
            .platform_administrators
            .bootstrap_root(&server.root_user_id)
            .await
            .expect("reconcile root platform administrator");
        let system_token = server
            .state
            .iam
            .service
            .issue_system_token(&server.root_user_id, &system_org_id)
            .expect("issue system-scope token");

        let auth = || server.auth_header();
        let response = server
            .client
            .post(format!(
                "{}/api/v1/ingest/logs/{}",
                server.base_url, MOLESIGNAL_SYSTEM_STREAM
            ))
            .header(auth().0, auth().1)
            .json(&json!({"message": "blocked"}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 403, "native ingest must be forbidden");

        let record = LogRecord {
            time_unix_nano: TimestampMicros::now().0 as u64 * 1_000,
            body: Some(AnyValue {
                value: Some(any_value::Value::StringValue("blocked".into())),
            }),
            ..Default::default()
        };
        let scope = ScopeLogs {
            log_records: vec![record],
            ..Default::default()
        };
        let resource = ResourceLogs {
            scope_logs: vec![scope],
            ..Default::default()
        };
        let otlp = ExportLogsServiceRequest {
            resource_logs: vec![resource],
        };
        let response = server
            .client
            .post(format!("{}/api/v1/logs", server.base_url))
            .header(auth().0, auth().1)
            .header("content-type", "application/x-protobuf")
            .header("stream-name", MOLESIGNAL_SYSTEM_STREAM)
            .body(otlp.encode_to_vec())
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 403, "OTLP/HTTP must be forbidden");

        let prom = PromWriteRequest {
            timeseries: vec![PromTimeSeries {
                labels: vec![PromLabel {
                    name: "__name__".into(),
                    value: MOLESIGNAL_SYSTEM_STREAM.into(),
                }],
                samples: vec![PromSample {
                    value: 1.0,
                    timestamp: TimestampMicros::now().0 / 1_000,
                }],
            }],
        };
        let compressed = snap::raw::Encoder::new()
            .compress_vec(&prom.encode_to_vec())
            .unwrap();
        let response = server
            .client
            .post(format!(
                "{}/api/v1/prometheus/api/v1/write",
                server.base_url
            ))
            .header(auth().0, auth().1)
            .header("content-type", "application/x-protobuf")
            .header("content-encoding", "snappy")
            .body(compressed)
            .send()
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            403,
            "Prometheus remote_write must be forbidden"
        );

        for (path, body) in [
            (
                "/api/v1/_bulk",
                "{\"index\":{}}\n{\"message\":\"blocked\"}\n",
            ),
            (
                "/api/v1/loki/api/v1/push",
                "{\"streams\":[{\"stream\":{},\"values\":[[\"1000\",\"blocked\"]]}]}",
            ),
        ] {
            let response = server
                .client
                .post(format!("{}{}", server.base_url, path))
                .header(auth().0, auth().1)
                .header("stream-name", MOLESIGNAL_SYSTEM_STREAM)
                .body(body)
                .send()
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                403,
                "compatibility ingest {path} must be forbidden"
            );
        }

        use molesignal::protocol::ingest::v1::{
            PushRequest, StreamType as ProtoStreamType,
            ingest_service_server::IngestService as IngestRpc,
        };
        let grpc_error = IngestRpc::push(
            &IngestGrpc::new(server.state.ingestion.clone()),
            Request::new(PushRequest {
                batch_id: String::new(),
                org_id: server.root_org_id.0.clone(),
                stream: MOLESIGNAL_SYSTEM_STREAM.into(),
                stream_type: ProtoStreamType::Logs as i32,
                payload: serde_json::to_vec(&vec![RawEvent {
                    timestamp: TimestampMicros::now(),
                    fields: Map::new(),
                }])
                .unwrap()
                .into(),
                received_at_micros: TimestampMicros::now().0,
            }),
        )
        .await
        .unwrap_err();
        assert_eq!(grpc_error.code(), tonic::Code::PermissionDenied);

        let response = server
            .client
            .post(format!("{}/api/v1/streams", server.base_url))
            .header(auth().0, auth().1)
            .json(&json!({
                "name": MOLESIGNAL_SYSTEM_STREAM,
                "stream_type": "logs"
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 403, "stream create must be forbidden");
        let system_trace = server
            .state
            .telemetry
            .streams
            .get(&system_org_id, MOLESIGNAL_SYSTEM_STREAM, StreamType::Traces)
            .await
            .unwrap();
        let response = server
            .client
            .put(format!(
                "{}/api/v1/streams/{}/settings",
                server.base_url, system_trace.id.0
            ))
            .header(auth().0, auth().1)
            .json(&json!({"retention_days": 3}))
            .send()
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            404,
            "tenant scope must not discover the stream during update"
        );
        let response = server
            .client
            .delete(format!(
                "{}/api/v1/streams/{}",
                server.base_url, system_trace.id.0
            ))
            .header(auth().0, auth().1)
            .send()
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            404,
            "tenant scope must not discover the stream during delete"
        );
        let response = server
            .client
            .get(format!(
                "{}/api/v1/streams/{}",
                server.base_url, system_trace.id.0
            ))
            .header(auth().0, auth().1)
            .send()
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            404,
            "tenant scope must not discover the system stream"
        );
        let response = server
            .client
            .get(format!(
                "{}/api/v1/streams/{}",
                server.base_url, system_trace.id.0
            ))
            .bearer_auth(&system_token)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200, "system scope may read the stream");
        assert!(
            server
                .state
                .telemetry
                .streams
                .list(&server.root_org_id)
                .await
                .unwrap()
                .iter()
                .all(|stream| stream.name != MOLESIGNAL_SYSTEM_STREAM),
            "tenant organization must not contain `_molesignal`"
        );

        let resource = server
            .state
            .telemetry
            .self_telemetry_resource
            .clone()
            .expect("self telemetry resource identity");
        let span = tracing::info_span!(
            target: "molesignal::it::self_telemetry",
            "self_telemetry.integration",
            otel.kind = "internal",
            signal = "traces"
        );
        span.in_scope(|| {
            tracing::info!(target: "molesignal::it::self_telemetry", "span event");
        });
        drop(span);
        register_int_counter(
            "self_telemetry_integration_counter",
            "Integration-test metric for self ingestion.",
        )
        .inc();

        let captured = server
            .state
            .telemetry
            .profiling_service
            .capture_cpu(1)
            .await
            .unwrap();
        let public_profile = captured.raw_pprof.clone();
        let response = server
            .client
            .post(format!(
                "{}/api/v1/profiles/upload?service=public-it&type=cpu",
                server.base_url
            ))
            .header(auth().0, auth().1)
            .header("stream-name", MOLESIGNAL_SYSTEM_STREAM)
            .body(public_profile)
            .send()
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            202,
            "public profile upload should stay accepted on profiles/default"
        );
        let runtime = server
            .state
            .telemetry
            .self_telemetry_runtime
            .clone()
            .expect("self telemetry runtime");
        runtime.persist_profile(captured).await.unwrap();
        runtime.stop_and_flush().await;

        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(2)
            .connect(&server.settings.store.meta.dsn)
            .await
            .unwrap();
        let flushed_org_id = system_org_id.0.clone();
        assert!(
            wait_until_async(15, move || {
                let pool = pool.clone();
                let org_id = flushed_org_id.clone();
                async move {
                    let row: (i64,) = sqlx::query_as(
                        "SELECT COUNT(*) FROM parquet_file_meta
                         WHERE org_id = $1 AND deleted = FALSE",
                    )
                    .bind(org_id)
                    .fetch_one(&pool)
                    .await
                    .unwrap_or((0,));
                    row.0 >= 3
                }
            })
            .await,
            "three typed system streams did not flush within timeout"
        );

        for stream_type in [
            StreamType::Metrics,
            StreamType::Traces,
            StreamType::Profiles,
        ] {
            assert!(
                query_count(&server, &system_token, system_org_id.as_str(), stream_type,).await
                    >= 1,
                "{stream_type:?}/_molesignal should be queryable"
            );
            let definition = server
                .state
                .telemetry
                .streams
                .get(&system_org_id, MOLESIGNAL_SYSTEM_STREAM, stream_type)
                .await
                .unwrap();
            assert_eq!(definition.retention.unwrap().days, 7);
        }
        assert!(
            query_count_for_name(
                &server,
                &server.root_token,
                server.root_org_id.as_str(),
                StreamType::Profiles,
                "default",
            )
            .await
                >= 1,
            "public profile upload must remain fixed to profiles/default"
        );

        let response = server
            .client
            .post(format!("{}/api/v1/query", server.base_url))
            .bearer_auth(&system_token)
            .json(&json!({
                "org_id": system_org_id.0,
                "language": "sql",
                "statement": "SELECT \"service.name\", \"node.id\", \"service.instance.id\" FROM _molesignal LIMIT 1",
                "time_range": {
                    "start": 0,
                    "end": TimestampMicros::now().0 + 10_000_000
                },
                "stream": {
                    "name": MOLESIGNAL_SYSTEM_STREAM,
                    "stream_type": "logs"
                }
            }))
            .send()
            .await
            .unwrap();
        let identity: Value = response.json().await.unwrap();
        assert_eq!(identity["rows"][0][0], "molesignal");
        assert_eq!(identity["rows"][0][1], "trace-e2e-node");
        assert_eq!(identity["rows"][0][2], resource.service_instance_id());
        assert!(has_profile_archive(server.object_store_root.path()));
    }
}
