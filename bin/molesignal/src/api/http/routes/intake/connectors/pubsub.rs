// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `POST /api/v1/_gcp_pubsub`: a Google Cloud Pub/Sub push subscription, one message per request.
//!
//! The push endpoint carries the connector's token as `?token=`. A connector that names
//! `oidcServiceAccount` also needs the subscription's OIDC token in `Authorization: Bearer`,
//! checked by [`oidc`] (and `oidcAudience` when it names one).
//!
//! The message becomes a log event in `targetStream` (default `gcp`); see
//! [`crate::infra::connectors::gcp::pubsub`]. Pub/Sub counts `102`, `200`, `201`, `202` and `204`
//! as an acknowledgement and delivers anything else again, so:
//!
//! - a message that cannot be read is acknowledged with `204` and logged: delivering it again would
//!   not make it readable, and would hold up the subscription;
//! - a write that fails for now (the buffer is full, the store is down) is not acknowledged, so
//!   Pub/Sub retries it with its backoff.
//!
//! Pub/Sub keeps `messageId` across redeliveries: a message written in the last [`SEEN_TTL_SECS`]
//! seconds is acknowledged without writing it again.

use std::{sync::OnceLock, time::Duration};

use axum::{
    body::Bytes,
    extract::{RawQuery, State},
    http::{HeaderMap, StatusCode, header::AUTHORIZATION},
};
use domain::stream::StreamType;
use kernel::{Error, Result};

use super::{
    MAX_BODY_BYTES, Refusals, decode_body, intake_events, resolve_connector, stream_of, token_from,
    touch,
};
use crate::{
    api::AppState,
    infra::connectors::{
        Connector, ConnectorKind,
        gcp::pubsub::{self, oidc},
    },
};

/// How long a written message is remembered.
const SEEN_TTL_SECS: u64 = 30 * 60;

fn seen() -> &'static moka::sync::Cache<String, ()> {
    static SEEN: OnceLock<moka::sync::Cache<String, ()>> = OnceLock::new();

    SEEN.get_or_init(|| {
        moka::sync::Cache::builder()
            .max_capacity(200_000)
            .time_to_live(Duration::from_secs(SEEN_TTL_SECS))
            .build()
    })
}

pub(super) async fn receive(
    State(state): State<AppState>,
    RawQuery(raw_query): RawQuery,
    headers: HeaderMap,
    body: Bytes,
) -> Result<StatusCode> {
    let token = token_from(&headers, raw_query.as_deref());
    let connector = resolve_connector(&state, ConnectorKind::GcpPubsubPush, token).await?;

    authenticate(&state, &connector, &headers).await?;

    let raw = decode_body(&headers, &body, MAX_BODY_BYTES)?;
    let header = |name: &str| headers.get(name).and_then(|value| value.to_str().ok());
    let unwrapped = header("x-goog-pubsub-message-id")
        .map(|id| (id, header("x-goog-pubsub-publish-time").unwrap_or_default()));

    let pushed = match pubsub::pushed(&raw, unwrapped) {
        Ok(pushed) => pushed,
        Err(error) => {
            tracing::warn!(connector = %connector.id.0, %error, "pub/sub push message left out");
            return Ok(StatusCode::NO_CONTENT);
        }
    };

    let seen_key = format!("{}:{}", connector.id.0, pushed.message_id);

    if !pushed.message_id.is_empty() && seen().contains_key(&seen_key) {
        return Ok(StatusCode::NO_CONTENT);
    }

    let refusals = match pubsub::event(&pushed) {
        Some(event) => {
            let stream = stream_of(&connector, "targetStream", "gcp");

            intake_events(
                &state,
                &connector,
                StreamType::LOGS,
                &stream,
                vec![event],
                raw.len(),
            )
            .await?
        }
        None => Refusals::default(),
    };

    if !pushed.message_id.is_empty() {
        seen().insert(seen_key, ());
    }

    touch(&state, &connector, refusals).await;
    Ok(StatusCode::NO_CONTENT)
}

/// Checks the subscription's OIDC token when the connector names a service account.
async fn authenticate(state: &AppState, connector: &Connector, headers: &HeaderMap) -> Result<()> {
    let setting = |key: &str| {
        connector
            .config_json
            .get(key)
            .and_then(|value| value.as_str())
            .map(str::trim)
            .unwrap_or_default()
    };

    let service_account = setting("oidcServiceAccount");

    if service_account.is_empty() {
        return Ok(());
    }

    let token = headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .ok_or_else(|| {
            Error::unauthorized(
                "this connector checks the subscription's OIDC token: turn authentication on for \
                 the push subscription",
            )
        })?;

    let expected = oidc::Expected {
        service_account,
        audience: setting("oidcAudience"),
    };

    oidc::verify(token, &expected, &state.iam.sso_jwks_cache).await
}
