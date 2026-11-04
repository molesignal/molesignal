// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `POST /api/v1/rum/events`: the one place the SDKs send events to.
//!
//! The body is a JSON array of events (a single object and one event per line are accepted too),
//! optionally gzip-compressed. The shape of an event is the contract in `contracts/rum/events/`.
//! Events are checked one by one, so a bad one is refused alone and the answer says which.

use std::{collections::BTreeMap, net::SocketAddr};

use axum::{
    Extension, Json,
    body::Bytes,
    extract::{ConnectInfo, State},
    http::{HeaderMap, HeaderName, header},
};
use serde_json::Value;

use super::symbolication::translate_events;
use crate::{
    api::{AppState, http::client_ip::ClientIpResolverHandle},
    app::{
        iam::IamContext,
        intake::usage::{IntakeReceipt, ServiceTally, split_by_events},
    },
    domain::{
        iam::permission,
        intake::usage::source::{IntakeCredential, UNNAMED_SERVICE},
        rum::event::{RumEventType, RumStream},
        stream::StreamType,
    },
    infra::rum::intake::{Decoded, DecodedBody, Report, Request, decode_body},
    shared::Result,
};

#[permission("rum.write")]
pub(super) async fn receive(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Extension(resolver): Extension<ClientIpResolverHandle>,
    peer: Option<Extension<ConnectInfo<SocketAddr>>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<Report>> {
    let DecodedBody { mut events, bytes } = decode_body(
        &body,
        text(&headers, &header::CONTENT_ENCODING),
        text(&headers, &header::CONTENT_TYPE),
    )?;

    let receipts = receipts_of(&ctx, &events, bytes as u64);

    translate_events(&state, &ctx, &mut events).await;

    let peer = peer.map(|Extension(ConnectInfo(address))| address);
    let ip = resolver.resolve(&headers, peer);

    let origin = state
        .telemetry
        .rum_intake
        .origin(ip, text(&headers, &header::USER_AGENT));

    let report = state
        .telemetry
        .rum_intake
        .intake(Request {
            org_id: &ctx.org_id,
            bound_application: ctx.credential_application_id.as_deref(),
            events,
            origin,
        })
        .await?;

    // Every kind of event counts for its share of what was taken in. Events the application
    // does not keep, by its own choice, are not among them.
    let counts = receipts
        .iter()
        .map(|receipt| receipt.services.total())
        .collect::<Vec<_>>();

    let accepted = split_by_events(report.accepted as u64, &counts);

    for (receipt, accepted) in receipts.into_iter().zip(accepted) {
        state
            .platform
            .intake_meter
            .record(&ctx.org_id, receipt, accepted);
    }

    Ok(Json(report))
}

/// What a request brings, stream by stream: an event counts where its kind is kept, and for the
/// service it names or else the application the credential is bound to. An event of no known
/// kind is refused later, and counts for nothing.
fn receipts_of(ctx: &IamContext, events: &[Decoded], bytes: u64) -> Vec<IntakeReceipt> {
    let mut services = BTreeMap::<&'static str, ServiceTally>::new();

    for event in events.iter().flatten() {
        let Some(stream) = event
            .get("type")
            .and_then(Value::as_str)
            .and_then(RumEventType::parse)
            .and_then(stream_of)
        else {
            continue;
        };

        let service = event
            .get("service")
            .and_then(Value::as_str)
            .or(ctx.credential_application_id.as_deref())
            .unwrap_or(UNNAMED_SERVICE);

        services.entry(stream).or_default().add(service, 1);
    }

    let counts = services
        .values()
        .map(ServiceTally::total)
        .collect::<Vec<_>>();
    let shares = split_by_events(bytes, &counts);

    services
        .into_iter()
        .zip(shares)
        .map(|((stream, services), bytes)| IntakeReceipt {
            credential: IntakeCredential::of(ctx),
            stream_type: StreamType::LOGS,
            stream: stream.to_string(),
            bytes,
            services,
        })
        .collect()
}

fn stream_of(kind: RumEventType) -> Option<&'static str> {
    RumStream::ALL
        .into_iter()
        .find(|stream| stream.event_type() == Some(kind))
        .map(RumStream::name)
}

fn text<'a>(headers: &'a HeaderMap, name: &HeaderName) -> Option<&'a str> {
    headers.get(name).and_then(|value| value.to_str().ok())
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;

    use super::*;

    fn context(application: Option<&str>) -> IamContext {
        IamContext {
            user_id: crate::shared::ids::Id::from_string("u-1"),
            org_id: crate::shared::ids::Id::from_string("org-1"),
            display_role: String::new(),
            roles: Vec::new(),
            credential_token_id: Some(crate::shared::ids::Id::from_string("t-1")),
            credential_application_id: application.map(str::to_string),
            credential_service_account_id: None,
            scope: crate::domain::iam::IamScope::Organization,
            permissions: Default::default(),
            policy_version: 0,
        }
    }

    #[test]
    fn events_count_in_the_stream_of_their_kind_and_for_the_service_they_name() {
        let events: Vec<Decoded> = vec![
            Ok(serde_json::json!({"type": "view", "service": "shop"})),
            Ok(serde_json::json!({"type": "view"})),
            Ok(serde_json::json!({"type": "error", "service": "shop"})),
            Ok(serde_json::json!({"type": "nonsense"})),
            Err("not JSON".to_string()),
        ];

        let receipts = receipts_of(&context(Some("web-app")), &events, 90);
        let streams = receipts
            .iter()
            .map(|receipt| receipt.stream.as_str())
            .collect::<Vec<_>>();

        assert_eq!(streams, ["rum_errors", "rum_views"]);
        assert_eq!(
            receipts.iter().map(|receipt| receipt.bytes).sum::<u64>(),
            90
        );
        assert_eq!(receipts[0].bytes, 30);
        assert_eq!(receipts[1].bytes, 60);
        assert_eq!(receipts[0].credential.id, "t-1");

        let mut expected = ServiceTally::single("shop", 1);
        expected.add("web-app", 1);

        assert_eq!(receipts[1].services, expected);
    }

    #[test]
    fn a_credential_bound_to_no_application_leaves_the_service_unnamed() {
        let events: Vec<Decoded> = vec![Ok(serde_json::json!({"type": "view"}))];
        let receipts = receipts_of(&context(None), &events, 10);

        assert_eq!(
            receipts[0].services,
            ServiceTally::single(UNNAMED_SERVICE, 1)
        );
    }

    #[test]
    fn a_header_is_its_text_or_nothing() {
        let mut headers = HeaderMap::new();

        headers.insert(header::CONTENT_ENCODING, HeaderValue::from_static("gzip"));
        headers.insert(
            header::USER_AGENT,
            HeaderValue::from_bytes(&[0xff, 0xfe]).unwrap(),
        );

        assert_eq!(text(&headers, &header::CONTENT_ENCODING), Some("gzip"));
        assert_eq!(text(&headers, &header::USER_AGENT), None);
        assert_eq!(text(&headers, &header::CONTENT_TYPE), None);
    }
}
