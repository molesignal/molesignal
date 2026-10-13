// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 渲染报表页时浏览器用的登录态。
//!
//! PDF/PNG 报表是无头浏览器加载前端的报表页打印出来的，页面要登录之后才能取数。renderer 在
//! 加载页面之前把一段登录态写进浏览器的 `localStorage['molesignal-auth']`，页面就以其中的身份
//! 向 API 取数，打印完立即清除。登录态里的 token 只在一次渲染所需的时间内有效。

use async_trait::async_trait;

use crate::{
    app::iam::{IamContext, IamService},
    infra::persistence::repositories::scheduled_reports::ScheduledReport,
    shared::{Error, Result},
};

/// 渲染用 token 的有效期。一次渲染最长 60 秒，其余留给排队等待空闲的浏览器实例。
pub const RENDER_SESSION_TTL_SECS: u64 = 600;

/// 定时投递渲染一份报表时，浏览器用的登录态从哪里来。
#[async_trait]
pub trait ReportRenderSession: Send + Sync {
    /// 给 `report` 的一次渲染签发登录态。
    ///
    /// 没有可用的身份时返回错误。错误文本原样写进这次运行的投递记录，所以要说清是谁、
    /// 为什么不可用、怎样恢复。
    async fn issue(&self, report: &ScheduledReport) -> Result<String>;
}

/// 给 `context` 里的用户签发一次渲染用的登录态：即时导出和分享快照以当前用户的身份渲染。
pub fn render_session(iam: &IamService, context: &IamContext) -> Result<String> {
    let token =
        iam.issue_short_lived_token(&context.user_id, &context.org_id, RENDER_SESSION_TTL_SECS)?;

    browser_session(&token, context)
}

/// 前端 auth store 持久化在浏览器里的 JSON（zustand persist）：token，加上页面用来显示的
/// 身份信息。授权不读这些显示信息，每次请求由后端按 token 重新解析。
pub(super) fn browser_session(token: &str, context: &IamContext) -> Result<String> {
    serde_json::to_string(&serde_json::json!({
        "state": {
            "token": token,
            "ctx": {
                "user_id": context.user_id.0,
                "org_id": context.org_id.0,
                "display_role": context.display_role,
                "roles": context.roles,
                "scope": context.scope,
            }
        },
        "version": 0
    }))
    .map_err(|error| Error::internal(format!("serialize report browser session: {error}")))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use serde_json::Value;

    use super::*;
    use crate::{
        app::iam::test_support::iam_service,
        domain::iam::{IamAssignedRole, IamScope},
        shared::ids::Id,
    };

    fn context() -> IamContext {
        IamContext {
            user_id: Id::from_string("user-1"),
            org_id: Id::from_string("org-1"),
            display_role: "Editor".into(),
            roles: vec![IamAssignedRole {
                id: Id::from_string("role-editor"),
                key: "editor".into(),
                name: "Editor".into(),
                builtin: true,
            }],
            credential_role_id: None,
            credential_application_id: None,
            credential_service_account_id: None,
            scope: IamScope::Organization,
            permissions: BTreeSet::new(),
            policy_version: 3,
        }
    }

    #[test]
    fn the_session_is_what_the_web_auth_store_persists() {
        let session: Value =
            serde_json::from_str(&browser_session("jwt", &context()).unwrap()).unwrap();

        assert_eq!(
            session,
            serde_json::json!({
                "state": {
                    "token": "jwt",
                    "ctx": {
                        "user_id": "user-1",
                        "org_id": "org-1",
                        "display_role": "Editor",
                        "roles": [
                            { "id": "role-editor", "key": "editor", "name": "Editor", "builtin": true }
                        ],
                        "scope": "organization",
                    }
                },
                "version": 0
            })
        );
    }

    #[test]
    fn a_render_session_carries_a_token_for_its_user_that_expires_within_minutes() {
        let iam = iam_service(86_400);
        let session: Value =
            serde_json::from_str(&render_session(&iam, &context()).unwrap()).unwrap();
        let token = session["state"]["token"].as_str().unwrap();

        let verified = iam.verify_token(token).unwrap();
        assert_eq!(verified.user_id, Id::from_string("user-1"));
        assert_eq!(verified.org_id, Id::from_string("org-1"));
        assert_eq!(verified.scope, IamScope::Organization);

        let lifetime = crate::app::iam::test_support::token_lifetime_secs(token);
        assert_eq!(lifetime, RENDER_SESSION_TTL_SECS);
    }

    #[test]
    fn a_render_token_never_outlives_the_configured_session() {
        let iam = iam_service(120);
        let session: Value =
            serde_json::from_str(&render_session(&iam, &context()).unwrap()).unwrap();
        let token = session["state"]["token"].as_str().unwrap();

        assert_eq!(
            crate::app::iam::test_support::token_lifetime_secs(token),
            120
        );
    }
}
