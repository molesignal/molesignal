// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Org 级邮箱域白名单。
//!
//! `GET    /orgs/email-domains`           list（per-org）
//! `POST   /orgs/email-domains`           add（规范化 + 校验）
//! `DELETE /orgs/email-domains/{domain}`  remove
//!
//! 空名单 = 不限制。邀请创建与 SSO/SAML 自助开户经 [`ensure_email_allowed`] 校验，
//! 超出名单的邮箱域被拒（403）。

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    routing::get,
};
use serde::{Deserialize, Serialize};

use crate::{
    api::AppState,
    app::iam::IamContext,
    domain::iam::{email_domain_allowed, permission},
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/orgs/email-domains", get(list).post(add))
        .route(
            "/orgs/email-domains/{domain}",
            axum::routing::delete(remove),
        )
}

#[derive(Debug, Serialize)]
struct Resp {
    domains: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct AddReq {
    domain: String,
}

#[permission("org.settings.read")]
async fn list(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Resp>> {
    let domains = state.iam.email_domains.list(&ctx.org_id).await?;
    Ok(Json(Resp { domains }))
}

#[permission("org.settings.manage")]
async fn add(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(req): Json<AddReq>,
) -> Result<Json<Resp>> {
    let domain = normalize_domain(&req.domain)?;
    state
        .iam
        .email_domains
        .add(&ctx.org_id, &domain, TimestampMicros::now())
        .await?;
    let domains = state.iam.email_domains.list(&ctx.org_id).await?;
    Ok(Json(Resp { domains }))
}

#[permission("org.settings.manage")]
async fn remove(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(domain): Path<String>,
) -> Result<Json<Resp>> {
    // 删除按规范化后的值匹配（add 时也规范化），避免大小写 / 前导符导致删不掉。
    let domain = normalize_domain(&domain)?;
    state.iam.email_domains.delete(&ctx.org_id, &domain).await?;
    let domains = state.iam.email_domains.list(&ctx.org_id).await?;
    Ok(Json(Resp { domains }))
}

/// 规范化用户输入的域名：小写、去空白、剥前导 `@`/`.`；要求非空、含 `.`、无空格 / `@`。
fn normalize_domain(raw: &str) -> Result<String> {
    let d = raw
        .trim()
        .trim_start_matches('@')
        .trim_start_matches('.')
        .to_ascii_lowercase();
    if d.is_empty() || !d.contains('.') || d.contains(char::is_whitespace) || d.contains('@') {
        return Err(Error::invalid(
            "domain must be a bare hostname like example.com",
        ));
    }
    Ok(d)
}

/// 校验某邮箱是否被 `org_id` 的邮箱域白名单准入；不准入返回 403。
/// 空名单（org 未配置）放行——保持默认行为向后兼容。供邀请 / SSO 开户路径复用。
pub(crate) async fn ensure_email_allowed(state: &AppState, org_id: &Id, email: &str) -> Result<()> {
    let allowed = state.iam.email_domains.list(org_id).await?;
    if email_domain_allowed(email, &allowed) {
        Ok(())
    } else {
        Err(Error::forbidden(
            "email domain is not permitted to join this organization",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::normalize_domain;

    #[test]
    fn normalizes_and_validates() {
        assert_eq!(normalize_domain(" @Example.COM ").unwrap(), "example.com");
        assert_eq!(normalize_domain(".vendor.net").unwrap(), "vendor.net");
        // 无点 / 空 / 含空格 / 含 @ → 拒绝。
        assert!(normalize_domain("localhost").is_err());
        assert!(normalize_domain("   ").is_err());
        assert!(normalize_domain("a b.com").is_err());
        assert!(normalize_domain("x@y.com").is_err());
    }
}
