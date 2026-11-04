// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! SSO 登录端点。
//!
//! 浏览器跳转的登录（OIDC / OAuth 2.0 / SAML）：
//! - `GET /auth/sso/login`、`GET /auth/sso/saml/login`：找到服务，存下 `state` /
//!   `nonce` / `next` 后跳到身份源。
//! - `GET /auth/sso/callback`、`POST /auth/sso/saml/callback`：校验身份、开户或登录，
//!   把结果存成一次性交接凭据，303 回前端 `/signin?sso=<凭据>`。
//! - `POST /auth/sso/exchange`：前端用凭据换会话、待确认的关联或失败原因。
//!
//! 从发起到回调的每一步出错都回到前端登录页说明原因，不停在接口的 JSON 上。LDAP
//! 由前端直接提交账号密码（[`ldap`]）。
//!
//! 登录只按外部身份的绑定找账号（[`provision`]）；邮箱已有账号而身份未绑定时，要本人在发起
//! 登录的页面确认（[`linking`]）。公网账号、允许登录的组和单点登录新建账号能加入哪些组织
//! 的限制在 [`policy`]；账号页的已关联登录方式在 [`identities`]。管理员上线前走一遍真实登录、
//! 不留下任何账号变化的测试登录在 [`signin_test`]。
//!
//! Login 阶段没有 session 拿 org_id，按 `?provider_id=` / `?provider=` 解析；都不带
//! 且只有一个启用的同类服务时自动选它。

use axum::Router;
use serde::Deserialize;

use crate::{
    api::AppState,
    domain::iam::{SsoProvider, SsoProviderKind},
    shared::{Error, Result, ids::Id},
};

mod handoff;
mod identities;
mod ldap;
mod linking;
mod oauth;
mod policy;
mod providers;
mod provision;
mod saml;
mod signin_test;

pub(crate) use linking::{PendingLink, complete_link};
pub(crate) use policy::ensure_membership_allowed;
pub(crate) use providers::{PublicProviderResponse, redirect_providers_of};
pub(crate) use signin_test::has_passing_test;

pub fn routes() -> Router<AppState> {
    Router::new()
        .merge(oauth::routes())
        .merge(handoff::routes())
        .merge(linking::routes())
        .merge(identities::routes())
        .merge(ldap::routes())
        .merge(saml::routes())
        .merge(providers::routes())
        .merge(signin_test::routes())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LoginParams {
    /// 直接指定 provider 行 id；用于多 IdP / 多 org 场景。
    #[serde(default)]
    provider_id: Option<String>,
    /// 按 provider name 在全表里找 enabled 行；name 在同 org 内唯一，跨 org 时
    /// 可能命中多个 — 命中多个时要求改用 `provider_id`。
    #[serde(default)]
    provider: Option<String>,
    /// 登录后回到的站内页面。
    #[serde(default)]
    next: Option<String>,
}

/// 找到发起登录的服务，要求它启用且属于 `kinds` 之一。
async fn resolve_provider_for_login(
    state: &AppState,
    params: &LoginParams,
    kinds: &[SsoProviderKind],
) -> Result<SsoProvider> {
    if let Some(id) = params.provider_id.as_deref() {
        let provider = state
            .iam
            .sso_providers
            .get(&Id::from_string(id.to_string()))
            .await?;

        if !provider.enabled {
            return Err(Error::invalid(format!(
                "provider {} is disabled",
                provider.name
            )));
        }

        if !kinds.contains(&provider.kind) {
            return Err(Error::invalid(format!(
                "provider {} is {}; this sign-in endpoint does not handle it",
                provider.name,
                provider.kind.as_str()
            )));
        }

        return Ok(provider);
    }

    let mut candidates = Vec::new();

    for kind in kinds {
        candidates.extend(state.iam.sso_providers.list_enabled_by_kind(*kind).await?);
    }

    if let Some(name) = params.provider.as_deref() {
        candidates.retain(|candidate| candidate.name == name);
    }

    match candidates.len() {
        0 => Err(Error::invalid(
            "no enabled SSO provider matches this sign-in request",
        )),
        1 => Ok(candidates.remove(0)),
        _ => {
            let names = candidates
                .iter()
                .map(|candidate| candidate.name.as_str())
                .collect::<Vec<_>>();

            Err(Error::invalid(format!(
                "multiple enabled SSO providers ({}); add ?provider_id=<id> to choose",
                names.join(", ")
            )))
        }
    }
}

/// 登录后要回到的站内路径；不是站内绝对路径的一律丢弃，防开放重定向。
fn safe_next(next: Option<&str>) -> Option<String> {
    let next = next?.trim();

    let local = next.starts_with('/')
        && !next.starts_with("//")
        && !next.contains('\\')
        && !next.chars().any(char::is_control)
        && next.len() <= 2048;

    local.then(|| next.to_owned())
}

/// URL 安全的随机串，用作 `state`、`nonce` 和交接凭据。
fn random_token() -> String {
    use base64::Engine as _;
    use rand::TryRng as _;

    let mut buf = [0u8; 32];
    rand::rngs::SysRng.try_fill_bytes(&mut buf).expect("os rng");
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(buf)
}

#[cfg(test)]
mod tests {
    use super::safe_next;

    #[test]
    fn only_local_paths_are_kept_as_next() {
        assert_eq!(safe_next(Some("/logs?q=1")).as_deref(), Some("/logs?q=1"));
        assert_eq!(safe_next(Some("//evil.example/x")), None);
        assert_eq!(safe_next(Some("/\\evil.example")), None);
        assert_eq!(safe_next(Some("https://evil.example")), None);
        assert_eq!(safe_next(Some("logs")), None);
        assert_eq!(safe_next(None), None);
    }
}
