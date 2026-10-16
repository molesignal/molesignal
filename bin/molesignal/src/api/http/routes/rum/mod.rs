// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! RUM receivers。
//!
//! Datadog-RUM 兼容 JSON 入口。Sessions / actions / errors 走通用 intake 路径
//! （`rum_sessions / rum_actions / rum_errors` 三个 stream，stream_type = Logs）；
//! replay 单独走 `RumReplayWriter`（object_store + `rum_replay_events` 元数据）。
//!
//! 不实装前端 SDK 与 player —— 只负责协议接收、存储与专用 Parquet 读模型接口。

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
        intake::{IntakeBatch, IntakeResult},
        rum::validate_application_id,
        stream::StreamType,
    },
    infra::{enrichment::GeoLookup, rum::normalize},
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

mod list;
mod query;
mod read_models;
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
        .merge(read_models::routes())
}

async fn intake_stream(
    state: &AppState,
    ctx: &IamContext,
    stream: &str,
    payload: Value,
) -> Result<IntakeResult> {
    let events = normalize::flatten(payload)?;

    let batch = IntakeBatch {
        batch_id: Id::new(),
        org_id: ctx.org_id.clone(),
        stream: stream.to_string(),
        stream_type: StreamType::LOGS,
        events,
        received_at: TimestampMicros::now(),
    };

    state.intake.intake(batch).await
}

#[permission("rum.write")]
async fn receive_sessions(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Extension(resolver): Extension<ClientIpResolverHandle>,
    peer: Option<Extension<ConnectInfo<SocketAddr>>>,
    headers: HeaderMap,
    Json(mut body): Json<Value>,
) -> Result<Json<IntakeResult>> {
    bind_application(&mut body, &ctx)?;
    let client_ip = resolve_client_ip(&resolver, &headers, peer);
    stamp_client_origin(&mut body, client_ip, state.telemetry.geoip.as_ref());

    Ok(Json(
        intake_stream(&state, &ctx, "rum_sessions", body).await?,
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
) -> Result<Json<IntakeResult>> {
    bind_application(&mut body, &ctx)?;
    let client_ip = resolve_client_ip(&resolver, &headers, peer);
    stamp_client_origin(&mut body, client_ip, state.telemetry.geoip.as_ref());

    Ok(Json(
        intake_stream(&state, &ctx, "rum_actions", body).await?,
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
) -> Result<Json<IntakeResult>> {
    bind_application(&mut body, &ctx)?;
    let client_ip = resolve_client_ip(&resolver, &headers, peer);
    stamp_client_origin(&mut body, client_ip, state.telemetry.geoip.as_ref());
    let translated = symbolication::translate_body(&state, &ctx, body).await;

    Ok(Json(
        intake_stream(&state, &ctx, "rum_errors", translated).await?,
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

/// 客户端上报值永不可信：先删除全部 IP 兼容别名，再只写入服务端解析的规范字段。
///
/// 国家同样以服务端为准：按服务端解析的 IP 查到国家时覆盖客户端上报值；查不到（没有 GeoIP 库、
/// 没有 IP、库里没有这个 IP）时保留客户端自带的 `country`，与未接 GeoIP 时一致。同一请求的
/// 事件来自同一个客户端，只查一次。
fn stamp_client_origin(body: &mut Value, resolved: Option<IpAddr>, geo: &dyn GeoLookup) {
    let country = resolved
        .and_then(|ip| geo.lookup(ip))
        .and_then(|location| location.country);

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

        if let Some(country) = &country {
            object.insert("country".into(), Value::String(country.clone()));
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
    use std::sync::atomic::{AtomicUsize, Ordering};

    use serde_json::json;

    use super::*;
    use crate::infra::enrichment::{GeoIp, GeoLocation};

    const KNOWN_IP: &str = "203.0.113.7";
    const UNKNOWN_IP: &str = "10.1.2.3";

    /// 假 GeoIP：只认识 `KNOWN_IP`（德国），并记下被查了几次。
    #[derive(Default)]
    struct FakeGeo {
        lookups: AtomicUsize,
    }

    impl GeoLookup for FakeGeo {
        fn lookup(&self, ip: IpAddr) -> Option<GeoLocation> {
            self.lookups.fetch_add(1, Ordering::Relaxed);

            (ip == KNOWN_IP.parse::<IpAddr>().unwrap()).then(|| GeoLocation {
                country: Some("DE".into()),
                ..GeoLocation::default()
            })
        }
    }

    fn ip(value: &str) -> Option<IpAddr> {
        Some(value.parse().unwrap())
    }

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

        stamp_client_origin(&mut body, ip("8.8.8.8"), &*GeoIp::noop());

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

        stamp_client_origin(&mut body, None, &*GeoIp::noop());
        assert!(body.get("ip_address").is_none());
        assert!(body.get("client_ip").is_none());
        assert!(body.get("ip").is_none());
    }

    #[test]
    fn country_found_for_the_server_ip_goes_on_every_event_with_one_lookup() {
        let geo = FakeGeo::default();
        let mut body = json!([{"session_id": "a"}, {"session_id": "b"}]);

        stamp_client_origin(&mut body, ip(KNOWN_IP), &geo);

        for event in body.as_array().unwrap() {
            assert_eq!(event["country"], "DE");
            assert_eq!(event["ip_address"], KNOWN_IP);
        }

        assert_eq!(geo.lookups.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn ip_without_a_country_adds_none() {
        let geo = FakeGeo::default();
        let mut body = json!({"session_id": "a"});

        stamp_client_origin(&mut body, ip(UNKNOWN_IP), &geo);

        assert!(body.get("country").is_none());
        assert_eq!(geo.lookups.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn missing_server_ip_skips_the_lookup() {
        let geo = FakeGeo::default();
        let mut body = json!({"session_id": "a"});

        stamp_client_origin(&mut body, None, &geo);

        assert!(body.get("country").is_none());
        assert_eq!(geo.lookups.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn server_country_overrides_the_client_value() {
        let mut body = json!([
            {"session_id": "a", "country": "US"},
            {"session_id": "b", "country": null}
        ]);

        stamp_client_origin(&mut body, ip(KNOWN_IP), &FakeGeo::default());

        for event in body.as_array().unwrap() {
            assert_eq!(event["country"], "DE");
        }
    }

    #[test]
    fn client_country_stays_when_the_server_finds_none() {
        let fake = FakeGeo::default();
        let missing_mmdb = GeoIp::noop();

        let cases: [(Option<IpAddr>, &dyn GeoLookup); 3] = [
            (ip(UNKNOWN_IP), &fake),
            (None, &fake),
            (ip(KNOWN_IP), &*missing_mmdb),
        ];

        for (resolved, geo) in cases {
            let mut body = json!({"session_id": "a", "country": "US"});

            stamp_client_origin(&mut body, resolved, geo);

            assert_eq!(body["country"], "US");
        }
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
