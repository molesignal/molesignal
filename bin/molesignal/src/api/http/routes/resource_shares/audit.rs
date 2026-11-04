// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 分享的审计，以及公开访问者留在审计和分享会话里的地址和浏览器。

use std::{convert::Infallible, net::SocketAddr};

use axum::{
    extract::{ConnectInfo, FromRequestParts},
    http::{header::USER_AGENT, request::Parts},
};
use serde_json::Value;

use crate::{
    api::{AppState, http::client_ip::ClientIpResolverHandle},
    infra::persistence::repositories::{audit_events::AuditEvent, resource_shares::ResourceShare},
    shared::{Result, ids::Id, time::TimestampMicros},
};

/// 公开访问者。地址按「客户端 IP 识别」解析，转发请求头只有连接对端是可信代理时才算数：否则谁都能
/// 在审计里留一个假地址，或者用超过列宽的内容让这条审计写不进去。
pub(super) struct ShareVisitor {
    pub(super) ip: Option<String>,
    pub(super) user_agent: Option<String>,
}

impl<S: Send + Sync> FromRequestParts<S> for ShareVisitor {
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
        let peer = parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|ConnectInfo(address)| *address);

        let ip = parts
            .extensions
            .get::<ClientIpResolverHandle>()
            .and_then(|resolver| resolver.resolve(&parts.headers, peer))
            .map(|ip| ip.to_string());

        let user_agent = parts
            .headers
            .get(USER_AGENT)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);

        Ok(Self { ip, user_agent })
    }
}

pub(super) async fn record_audit(
    state: &AppState,
    share: &ResourceShare,
    action: &str,
    actor_kind: &str,
    actor_id: &str,
    visitor: Option<&ShareVisitor>,
    payload: Value,
) -> Result<()> {
    state
        .iam
        .audit_events
        .record(AuditEvent {
            id: Id::new(),
            org_id: share.organization_id.clone(),
            actor_kind: actor_kind.into(),
            actor_id: actor_id.into(),
            action: action.into(),
            target_kind: Some("resource_share".into()),
            target_id: Some(share.id.0.clone()),
            ip: visitor.and_then(|visitor| visitor.ip.clone()),
            user_agent: visitor.and_then(|visitor| visitor.user_agent.clone()),
            payload,
            ts: TimestampMicros::now(),
        })
        .await
}

#[cfg(test)]
mod tests {
    use axum::http::Request;

    use super::*;
    use crate::domain::iam::{ClientIpMode, ClientIpResolverSettings};

    async fn visitor_from(
        resolver: ClientIpResolverHandle,
        peer: &str,
        forwarded: &str,
    ) -> ShareVisitor {
        let (mut parts, ()) = Request::builder()
            .header("x-forwarded-for", forwarded)
            .header(USER_AGENT, "test-browser/1.0")
            .body(())
            .unwrap()
            .into_parts();

        parts
            .extensions
            .insert(ConnectInfo(peer.parse::<SocketAddr>().unwrap()));
        parts.extensions.insert(resolver);

        ShareVisitor::from_request_parts(&mut parts, &())
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn a_forged_header_leaves_no_trace_in_the_address() {
        let too_long_for_the_column = "x".repeat(200);

        for forged in ["1.1.1.1", too_long_for_the_column.as_str()] {
            let visitor = visitor_from(ClientIpResolverHandle::peer(), "8.8.8.8:443", forged).await;

            assert_eq!(visitor.ip.as_deref(), Some("8.8.8.8"));
        }
    }

    #[tokio::test]
    async fn a_trusted_proxy_names_the_visitor_behind_it() {
        let resolver = ClientIpResolverHandle::new(&ClientIpResolverSettings {
            mode: ClientIpMode::ForwardedChain,
            header_name: "X-Forwarded-For".into(),
            trusted_proxy_cidrs: vec!["10.0.0.0/8".into()],
            ..ClientIpResolverSettings::default()
        })
        .unwrap();

        let visitor = visitor_from(resolver, "10.0.0.2:443", "8.8.8.8").await;

        assert_eq!(visitor.ip.as_deref(), Some("8.8.8.8"));
    }

    #[tokio::test]
    async fn a_private_address_is_written_down_though_it_is_no_signin_source() {
        let visitor = visitor_from(ClientIpResolverHandle::peer(), "10.0.0.1:443", "8.8.8.8").await;

        assert_eq!(visitor.ip.as_deref(), Some("10.0.0.1"));
    }

    #[tokio::test]
    async fn the_browser_is_written_down_as_it_says() {
        let visitor = visitor_from(ClientIpResolverHandle::peer(), "8.8.8.8:443", "1.1.1.1").await;

        assert_eq!(visitor.user_agent.as_deref(), Some("test-browser/1.0"));
    }
}
