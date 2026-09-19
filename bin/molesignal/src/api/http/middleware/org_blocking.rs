// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Org 停服拦截 middleware。
//!
//! 平台管理员停用的 org 一律拒绝（403）；计费部署下，订阅停服 / 取消、或试用到期
//! （且无可服务订阅）的 org，其数据面读写一律拒绝（402）。计费判定逻辑与写入门禁共用
//! [`crate::api::http::billing::org_blocked_cached`]
//! （同一 [`BillingStateCache`]，webhook 精确失效 + 短 TTL 兜底）。
//!
//! **不把人锁死**：登录 / 登出、计费配置（去付费）、健康检查，以及驱动"去付费"提示所
//! 必需的 app shell 元数据端点（`/me`、`/instance`）始终放行——否则欠费 org 连恢复入口
//! 都进不去。OSS / 未接计费部署：[`org_blocked_cached`] 直接返回未拦截，零开销。
//!
//! 装配顺序：挂在 `auth_layer` **之后**（更内层），以读取其注入的 [`IamContext`]。
//! 未鉴权请求（auth 白名单路径，无 IamContext）一律放行。
//!
//! [`BillingStateCache`]: crate::infra::caching::BillingStateCache
//! [`org_blocked_cached`]: crate::api::http::billing::org_blocked_cached

use axum::{
    extract::{Request, State},
    middleware::Next,
    response::Response,
};

use crate::{
    api::{AppState, http::billing::org_blocked_cached},
    app::iam::IamContext,
    shared::{Error, time::TimestampMicros},
};

/// 即便 org 已停服也必须放行的路径前缀（自助恢复 + app shell 启动）。
const ALLOW_PREFIXES: &[&str] = &[
    "/api/v1/auth/",            // 登录 / 登出 / 刷新
    "/api/v1/billing/",         // 查看试用态 / 配置付费 / webhook
    "/api/v1/healthz",          // 健康检查
    "/api/v1/me",               // app shell：当前用户（驱动"去付费"提示）
    "/api/v1/instance",         // app shell：实例配置 / edition
    "/api/v1/iam/capabilities", // app shell：动态导航与权限快照
];

fn is_recovery_path(path: &str) -> bool {
    if ALLOW_PREFIXES.iter().any(|prefix| path.starts_with(prefix)) || path == "/api/v1/orgs" {
        return true;
    }
    let Some(rest) = path.strip_prefix("/api/v1/orgs/") else {
        return false;
    };
    let mut segments = rest.split('/');
    matches!(
        (segments.next(), segments.next(), segments.next()),
        (Some(id), Some("select"), None) if !id.is_empty()
    )
}

pub async fn org_blocking_layer(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Result<Response, Error> {
    // 未鉴权请求（auth 白名单路径）无 IamContext：与计费无关，直接放行。
    let Some(org_id) = req
        .extensions()
        .get::<IamContext>()
        .map(|ctx| ctx.org_id.clone())
    else {
        return Ok(next.run(req).await);
    };

    // 自助恢复 / app shell 路径：即便已停服也放行。
    if is_recovery_path(req.uri().path()) {
        return Ok(next.run(req).await);
    }

    let organization = state.iam.service.orgs.get(&org_id).await?;
    organization.ensure_enabled()?;

    if org_blocked_cached(&state, &org_id, TimestampMicros::now().0).await? {
        return Err(Error::payment_required(
            "organization access is paused: the subscription is suspended or cancelled, \
             or the trial has expired. Visit billing to restore service.",
        ));
    }
    Ok(next.run(req).await)
}

#[cfg(test)]
mod tests {
    use super::is_recovery_path;

    #[test]
    fn organization_recovery_allowlist_does_not_expose_mutations() {
        assert!(is_recovery_path("/api/v1/orgs"));
        assert!(is_recovery_path("/api/v1/orgs/tenant-a/select"));
        assert!(!is_recovery_path("/api/v1/orgs/tenant-a/status"));
        assert!(!is_recovery_path("/api/v1/orgs/tenant-a/members"));
        assert!(!is_recovery_path("/api/v1/orgs/tenant-a/select/extra"));
    }
}
