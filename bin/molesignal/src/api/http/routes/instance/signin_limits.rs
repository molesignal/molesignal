// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 密码登录限速的平台设置（[`SigninLimits`]）。在平台组织的设置页修改（`sys.settings.signin.*`），
//! 改动记入平台审计；每次登录现读，改了立即生效。
//!
//! 回答里还带着管理员这次请求的来源：按来源计数要先取对地址，代理后面没配可信代理时所有人都是同一个
//! 来源。用自己的请求最容易看出来。

use std::net::SocketAddr;

use axum::{
    Extension, Json, Router,
    extract::{ConnectInfo, State},
    http::HeaderMap,
    routing::get,
};
use serde::Serialize;
use serde_json::json;

use super::data::audit;
use crate::{
    api::{
        AppState,
        http::client_ip::{ClientIpResolverHandle, ObservedSource},
    },
    app::iam::IamContext,
    domain::iam::{
        MAX_SIGNIN_LIMIT_FAILURES, MAX_SIGNIN_LIMIT_INTERVAL_SECS, MAX_SIGNIN_LIMIT_LOCK_MINUTES,
        SigninLimits, permission,
    },
    shared::{Error, Result, time::TimestampMicros},
};

pub(super) fn routes() -> Router<AppState> {
    Router::new().route(
        "/settings/signin_limits",
        get(get_signin_limits).put(put_signin_limits),
    )
}

#[derive(Debug, Serialize)]
struct SigninLimitsView {
    #[serde(flatten)]
    limits: SigninLimits,
    /// 这次请求在登录限速那里是什么样。
    source: ObservedSource,
}

#[permission("sys.settings.signin.read")]
async fn get_signin_limits(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Extension(resolver): Extension<ClientIpResolverHandle>,
    peer: Option<Extension<ConnectInfo<SocketAddr>>>,
    headers: HeaderMap,
) -> Result<Json<SigninLimitsView>> {
    let settings = state.iam.instance_settings.get().await?;
    let peer = peer.map(|Extension(ConnectInfo(address))| address);

    Ok(Json(SigninLimitsView {
        limits: settings.signin_limits,
        source: resolver.observe_signin_source(&headers, peer),
    }))
}

fn validated(limits: &SigninLimits) -> Result<()> {
    let counts = [
        limits.account_source_failures,
        limits.source_failures,
        limits.account_failures,
    ];

    if counts
        .iter()
        .any(|count| *count > MAX_SIGNIN_LIMIT_FAILURES)
    {
        return Err(Error::invalid(format!(
            "failure limits must be between 0 and {MAX_SIGNIN_LIMIT_FAILURES}"
        )));
    }

    if !(1..=MAX_SIGNIN_LIMIT_INTERVAL_SECS).contains(&limits.account_interval_secs) {
        return Err(Error::invalid(format!(
            "account_interval_secs must be between 1 and {MAX_SIGNIN_LIMIT_INTERVAL_SECS}"
        )));
    }

    if !(1..=MAX_SIGNIN_LIMIT_LOCK_MINUTES).contains(&limits.lock_minutes) {
        return Err(Error::invalid(format!(
            "lock_minutes must be between 1 and {MAX_SIGNIN_LIMIT_LOCK_MINUTES}"
        )));
    }

    Ok(())
}

#[permission("sys.settings.signin.manage")]
async fn put_signin_limits(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Extension(resolver): Extension<ClientIpResolverHandle>,
    peer: Option<Extension<ConnectInfo<SocketAddr>>>,
    headers: HeaderMap,
    Json(request): Json<SigninLimits>,
) -> Result<Json<SigninLimitsView>> {
    validated(&request)?;

    // read-modify-write：保留其它实例设置。
    let mut settings = state.iam.instance_settings.get().await?;
    let previous = settings.signin_limits;
    settings.signin_limits = request;
    settings.updated_at = TimestampMicros::now();
    let updated = state.iam.instance_settings.update(settings).await?;

    if updated.signin_limits != previous {
        audit(
            &state,
            &ctx,
            "signin_limits.update",
            json!({
                "signinLimits": updated.signin_limits,
                "previousSigninLimits": previous,
            }),
        )
        .await;
    }

    let peer = peer.map(|Extension(ConnectInfo(address))| address);

    Ok(Json(SigninLimitsView {
        limits: updated.signin_limits,
        source: resolver.observe_signin_source(&headers, peer),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_are_within_the_bounds() {
        assert!(validated(&SigninLimits::default()).is_ok());
    }

    #[test]
    fn a_count_can_be_turned_off_but_not_past_the_bound() {
        let off = SigninLimits {
            account_source_failures: 0,
            source_failures: 0,
            account_failures: 0,
            ..SigninLimits::default()
        };

        assert!(validated(&off).is_ok());

        let too_many = SigninLimits {
            source_failures: MAX_SIGNIN_LIMIT_FAILURES + 1,
            ..SigninLimits::default()
        };

        assert!(validated(&too_many).is_err());
    }

    #[test]
    fn the_account_pace_and_the_lock_stay_short() {
        for account_interval_secs in [0, MAX_SIGNIN_LIMIT_INTERVAL_SECS + 1] {
            let limits = SigninLimits {
                account_interval_secs,
                ..SigninLimits::default()
            };

            assert!(validated(&limits).is_err());
        }

        for lock_minutes in [0, MAX_SIGNIN_LIMIT_LOCK_MINUTES + 1] {
            let limits = SigninLimits {
                lock_minutes,
                ..SigninLimits::default()
            };

            assert!(validated(&limits).is_err());
        }
    }

    #[test]
    fn the_answer_flattens_the_limits_next_to_the_source() {
        let view = SigninLimitsView {
            limits: SigninLimits::default(),
            source: ObservedSource {
                address: Some("203.0.113.7".into()),
                counted: true,
                behind_unconfigured_proxy: false,
            },
        };

        assert_eq!(
            serde_json::to_value(view).unwrap(),
            json!({
                "accountSourceFailures": 5,
                "sourceFailures": 30,
                "accountFailures": 20,
                "accountIntervalSecs": 60,
                "lockMinutes": 15,
                "source": {
                    "address": "203.0.113.7",
                    "counted": true,
                    "behindUnconfiguredProxy": false,
                },
            })
        );
    }
}
