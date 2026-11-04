// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Session replay: `POST /api/v1/rum/replay` takes the segments an SDK records, and
//! `GET /api/v1/rum/replay/{sessionId}` gives them back to the player.

use std::net::SocketAddr;

use axum::{
    Extension, Json,
    extract::{ConnectInfo, Path, State},
    http::{HeaderMap, header},
};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::{
    api::{AppState, http::client_ip::ClientIpResolverHandle},
    app::{
        iam::IamContext,
        intake::usage::{IntakeReceipt, ServiceTally},
    },
    domain::{
        iam::permission,
        intake::usage::source::{IntakeCredential, RUM_REPLAY_SOURCE},
        rum::validate_application_id,
        stream::StreamType,
    },
    shared::{Error, Result},
};

/// A segment as the SDKs upload it.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ReplayBody {
    pub application: Option<String>,
    pub session_id: String,
    pub seq: i32,
    pub events: Vec<Value>,
    /// What an SDK says of the session it records, so that the filters of the application that
    /// look at where the session was run can tell whether it is kept. Without them a segment is
    /// kept when any filter that might describe the session would keep it.
    pub env: Option<String>,
    pub service: Option<String>,
    pub version: Option<String>,
    /// What kind of session it is, when a test drove it: such a session is not sampled.
    pub session_type: Option<String>,
}

#[permission("rum.write")]
pub(super) async fn receive(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Extension(resolver): Extension<ClientIpResolverHandle>,
    peer: Option<Extension<ConnectInfo<SocketAddr>>>,
    headers: HeaderMap,
    Json(body): Json<ReplayBody>,
) -> Result<Json<Value>> {
    let application = canonical_application(
        body.application.as_deref(),
        ctx.credential_application_id.as_deref(),
    )?;

    // Like an event, a segment is written to an application that exists.
    let found = state
        .telemetry
        .rum_applications
        .get(&ctx.org_id, &application)
        .await?;

    // A session the application does not keep has no replay either, by the same choice that left
    // its events out.
    let peer = peer.map(|Extension(ConnectInfo(address))| address);
    let origin = state.telemetry.rum_intake.origin(
        resolver.resolve(&headers, peer),
        headers
            .get(header::USER_AGENT)
            .and_then(|value| value.to_str().ok()),
    );

    if !state.telemetry.rum_intake.keeps_replay(
        &found.settings,
        &body.session_id,
        said(&body),
        &origin,
    ) {
        return Ok(Json(json!({
            "application": application,
            "sessionId": body.session_id,
            "seq": body.seq,
            "sampled": true,
        })));
    }

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

    // Replays are kept as files, in no stream of their own, and come from a credential like any
    // other data.
    state.platform.intake_meter.record(
        &ctx.org_id,
        IntakeReceipt {
            credential: IntakeCredential::of(&ctx),
            stream_type: StreamType::LOGS,
            stream: RUM_REPLAY_SOURCE.to_string(),
            bytes: rec.bytes_uncompressed,
            services: ServiceTally::single(&application, rec.event_count as u64),
        },
        rec.event_count as u64,
    );

    Ok(Json(json!({
        "id": rec.id.0,
        "application": rec.application_id,
        "sessionId": rec.session_id,
        "seq": rec.seq,
        "objectKey": rec.object_key,
        "bytesUncompressed": rec.bytes_uncompressed,
        "eventCount": rec.event_count,
        "hasFullSnapshot": rec.has_full_snapshot,
        "contentHash": rec.content_hash,
        "firstEventAt": rec.first_event_at_micros,
    })))
}

#[permission(any("rum.replay.read", "sys.telemetry.read"))]
pub(super) async fn read(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(session_id): Path<String>,
) -> Result<Json<Value>> {
    let (segment_count, events) = state
        .telemetry
        .rum_replay
        .get_session_events(&ctx.org_id, &session_id)
        .await?;

    Ok(Json(json!({
        "sessionId": session_id,
        "segmentCount": segment_count,
        "events": events,
    })))
}

/// What the segment says of its session: the environment, service, version and kind, when it names
/// them.
fn said(body: &ReplayBody) -> Map<String, Value> {
    [
        ("env", &body.env),
        ("service", &body.service),
        ("version", &body.version),
        ("session.type", &body.session_type),
    ]
    .into_iter()
    .filter_map(|(name, value)| {
        let value = value.as_deref()?.trim();

        (!value.is_empty()).then(|| (name.to_string(), Value::from(value)))
    })
    .collect()
}

/// The application a replay segment is written to. A token bound to an application writes to it
/// and to no other; one bound to none names the application.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_segment_says_what_it_names_of_its_session_and_nothing_else() {
        let body: ReplayBody = serde_json::from_value(json!({
            "sessionId": "s-1",
            "seq": 0,
            "events": [],
            "env": " prod ",
            "service": "  ",
            "version": "2.4.1"
        }))
        .unwrap();

        let said = said(&body);

        assert_eq!(said.get("env"), Some(&json!("prod")));
        assert_eq!(said.get("version"), Some(&json!("2.4.1")));
        assert!(!said.contains_key("service"));
        assert_eq!(said.len(), 2);
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

    #[test]
    fn a_credential_bound_to_none_names_the_application() {
        assert_eq!(
            canonical_application(Some("mobile-shop"), None).unwrap(),
            "mobile-shop"
        );
        assert!(canonical_application(Some("../x"), None).is_err());
    }
}
