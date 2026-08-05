// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! RUM receivers。
//!
//! Datadog-RUM 兼容 JSON 入口。Sessions / actions / errors 走通用 ingest 路径
//! （`rum_sessions / rum_actions / rum_errors` 三个 stream，stream_type = Logs）；
//! replay 单独走 `RumReplayWriter`（object_store + `rum_replay_events` 元数据）。
//!
//! 不实装前端 SDK 与 player —— 只负责协议接收 + 存储 + 后续查询接口（query/api/v1 走通用 SQL）。

use std::net::{IpAddr, SocketAddr};

use axum::{
    Extension, Json, Router,
    extract::{ConnectInfo, DefaultBodyLimit, Path, State},
    http::HeaderMap,
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::Value;

use crate::{
    api::{AppState, http::client_ip::ClientIpResolverHandle},
    app::iam::IamContext,
    domain::{
        iam::permission,
        ingestion::{IngestBatch, IngestResult},
        rum::validate_application_id,
        stream::StreamType,
    },
    infra::rum::normalize,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

mod list;
mod query;
mod symbolication;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/rum/sessions", post(receive_sessions))
        .route("/rum/actions", post(receive_actions))
        .route("/rum/errors", post(receive_errors))
        .route(
            "/rum/replay",
            post(receive_replay).layer(DefaultBodyLimit::max(
                crate::infra::rum::replay::MAX_REPLAY_SEGMENT_BYTES as usize,
            )),
        )
        .route("/rum/replay/{session_id}", get(read_replay))
        .merge(list::routes())
        .merge(query::routes())
}

async fn ingest_stream(
    state: &AppState,
    ctx: &IamContext,
    stream: &str,
    payload: Value,
) -> Result<IngestResult> {
    let events = normalize::flatten(payload)?;
    let batch = IngestBatch {
        batch_id: Id::new(),
        org_id: ctx.org_id.clone(),
        stream: stream.to_string(),
        stream_type: StreamType::Logs,
        events,
        received_at: TimestampMicros::now(),
    };
    state.ingestion.ingest(batch).await
}

#[permission("rum.write")]
async fn receive_sessions(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Extension(resolver): Extension<ClientIpResolverHandle>,
    peer: Option<Extension<ConnectInfo<SocketAddr>>>,
    headers: HeaderMap,
    Json(mut body): Json<Value>,
) -> Result<Json<IngestResult>> {
    bind_application(&mut body, &ctx)?;
    overwrite_client_ip_fields(&mut body, resolve_client_ip(&resolver, &headers, peer));
    Ok(Json(
        ingest_stream(&state, &ctx, "rum_sessions", body).await?,
    ))
}

#[permission("rum.write")]
async fn receive_actions(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Extension(resolver): Extension<ClientIpResolverHandle>,
    peer: Option<Extension<ConnectInfo<SocketAddr>>>,
    headers: HeaderMap,
    Json(mut body): Json<Value>,
) -> Result<Json<IngestResult>> {
    bind_application(&mut body, &ctx)?;
    overwrite_client_ip_fields(&mut body, resolve_client_ip(&resolver, &headers, peer));
    Ok(Json(
        ingest_stream(&state, &ctx, "rum_actions", body).await?,
    ))
}

#[permission("rum.write")]
async fn receive_errors(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Extension(resolver): Extension<ClientIpResolverHandle>,
    peer: Option<Extension<ConnectInfo<SocketAddr>>>,
    headers: HeaderMap,
    Json(mut body): Json<Value>,
) -> Result<Json<IngestResult>> {
    bind_application(&mut body, &ctx)?;
    overwrite_client_ip_fields(&mut body, resolve_client_ip(&resolver, &headers, peer));
    let translated = symbolication::translate_body(&state, &ctx, body).await;
    Ok(Json(
        ingest_stream(&state, &ctx, "rum_errors", translated).await?,
    ))
}

fn resolve_client_ip(
    resolver: &ClientIpResolverHandle,
    headers: &HeaderMap,
    peer: Option<Extension<ConnectInfo<SocketAddr>>>,
) -> Option<IpAddr> {
    let peer = peer.map(|Extension(ConnectInfo(address))| address);
    resolver.resolve(headers, peer)
}

/// 客户端上报值永不可信：先删除全部兼容别名，再只写入服务端解析的规范字段。
fn overwrite_client_ip_fields(body: &mut Value, resolved: Option<IpAddr>) {
    let events = match body {
        Value::Array(events) => events.as_mut_slice(),
        Value::Object(_) => std::slice::from_mut(body),
        _ => return,
    };
    for event in events {
        let Some(object) = event.as_object_mut() else {
            continue;
        };
        for key in ["ip_address", "client_ip", "ip"] {
            object.remove(key);
        }
        if let Some(ip) = resolved {
            object.insert("ip_address".into(), Value::String(ip.to_string()));
        }
    }
}

fn bind_application(body: &mut Value, context: &IamContext) -> Result<()> {
    let events = match body {
        Value::Array(events) => events.as_mut_slice(),
        Value::Object(_) => std::slice::from_mut(body),
        _ => return Err(Error::invalid("RUM payload must be an object or array")),
    };
    for event in events {
        let object = event
            .as_object_mut()
            .ok_or_else(|| Error::invalid("RUM payload item must be an object"))?;
        let supplied = object
            .get("application")
            .or_else(|| object.get("application_id"))
            .map(|value| {
                value
                    .as_str()
                    .ok_or_else(|| Error::invalid("RUM application must be a string"))
            })
            .transpose()?;
        let application =
            canonical_application(supplied, context.credential_application_id.as_deref())?;
        object.remove("application_id");
        object.insert("application".into(), Value::String(application));
    }
    Ok(())
}

fn canonical_application(value: Option<&str>, credential: Option<&str>) -> Result<String> {
    let supplied = value.map(validate_application_id).transpose()?;
    match (credential, supplied) {
        (Some(expected), Some(actual)) if expected != actual => Err(Error::forbidden(
            "RUM client token is bound to a different application",
        )),
        (Some(expected), _) => Ok(expected.to_string()),
        (None, Some(application)) => Ok(application.to_string()),
        (None, None) => Err(Error::invalid("RUM application is required")),
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplayBody {
    pub application: Option<String>,
    pub session_id: String,
    pub seq: i32,
    pub events: Vec<Value>,
}

#[permission("rum.write")]
async fn receive_replay(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(body): Json<ReplayBody>,
) -> Result<Json<Value>> {
    let application = canonical_application(
        body.application.as_deref(),
        ctx.credential_application_id.as_deref(),
    )?;
    let rec = state
        .telemetry
        .rum_replay
        .put_session_events(
            &ctx.org_id,
            &application,
            &body.session_id,
            body.seq,
            &body.events,
        )
        .await?;
    Ok(Json(serde_json::json!({
        "id": rec.id.0,
        "application": rec.application_id,
        "session_id": rec.session_id,
        "seq": rec.seq,
        "object_key": rec.object_key,
        "bytes_uncompressed": rec.bytes_uncompressed,
        "event_count": rec.event_count,
        "has_full_snapshot": rec.has_full_snapshot,
        "content_hash": rec.content_hash,
        "first_event_at_micros": rec.first_event_at_micros,
    })))
}

#[permission(any("streams.query", "sys.telemetry.read"))]
async fn read_replay(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(session_id): Path<String>,
) -> Result<Json<Value>> {
    let (segment_count, events) = state
        .telemetry
        .rum_replay
        .get_session_events(&ctx.org_id, &session_id)
        .await?;
    Ok(Json(serde_json::json!({
        "session_id": session_id,
        "segment_count": segment_count,
        "events": events,
    })))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn canonical_read_and_write_routes_merge_without_conflict() {
        let _ = routes();
    }

    #[test]
    fn server_ip_replaces_every_client_alias() {
        let mut body = json!([
            {"session_id": "a", "ip_address": "1.1.1.1", "client_ip": "2.2.2.2"},
            {"session_id": "b", "ip": "3.3.3.3"}
        ]);
        overwrite_client_ip_fields(&mut body, Some("8.8.8.8".parse().unwrap()));
        for event in body.as_array().unwrap() {
            assert_eq!(event["ip_address"], "8.8.8.8");
            assert!(event.get("client_ip").is_none());
            assert!(event.get("ip").is_none());
        }
    }

    #[test]
    fn missing_server_ip_still_removes_spoofed_values() {
        let mut body = json!({
            "session_id": "a",
            "ip_address": "1.1.1.1",
            "client_ip": "2.2.2.2",
            "ip": "3.3.3.3"
        });
        overwrite_client_ip_fields(&mut body, None);
        assert!(body.get("ip_address").is_none());
        assert!(body.get("client_ip").is_none());
        assert!(body.get("ip").is_none());
    }

    #[test]
    fn application_bound_credentials_cannot_switch_applications() {
        assert_eq!(
            canonical_application(None, Some("mobile-shop")).unwrap(),
            "mobile-shop"
        );
        assert!(canonical_application(Some("other-app"), Some("mobile-shop")).is_err());
        assert!(canonical_application(None, None).is_err());
    }
}
