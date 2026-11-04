// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 渲染报表页时浏览器用的登录态。
//!
//! PDF/PNG 报表是无头浏览器加载前端的报表页打印出来的，页面要登录之后才能取数。renderer 在
//! 加载页面之前把一个 access token 写进浏览器的会话 Cookie，页面就以它的身份向 API 取数，
//! 打印完立即清除。这个 token 只在一次渲染所需的时间内有效，也没有 refresh token。

use async_trait::async_trait;

use crate::{
    app::iam::{IamContext, IamService},
    infra::persistence::repositories::scheduled_reports::ScheduledReport,
    shared::Result,
};

/// 渲染用 token 的有效期。一次渲染最长 60 秒，其余留给排队等待空闲的浏览器实例。
pub const RENDER_SESSION_TTL_SECS: u64 = 600;

/// 定时投递渲染一份报表时，浏览器用的登录态从哪里来。
#[async_trait]
pub trait ReportRenderSession: Send + Sync {
    /// 给 `report` 的一次渲染签发 access token。
    ///
    /// 没有可用的身份时返回错误。错误文本原样写进这次运行的投递记录，所以要说清是谁、
    /// 为什么不可用、怎样恢复。
    async fn issue(&self, report: &ScheduledReport) -> Result<String>;
}

/// 给 `context` 里的用户签发一次渲染用的 access token：即时导出和分享快照以当前用户的身份
/// 渲染。
pub fn render_session(iam: &IamService, context: &IamContext) -> Result<String> {
    iam.issue_short_lived_token(&context.user_id, &context.org_id, RENDER_SESSION_TTL_SECS)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

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
            credential_token_id: None,
            credential_application_id: None,
            credential_service_account_id: None,
            scope: IamScope::Organization,
            permissions: BTreeSet::new(),
            policy_version: 3,
        }
    }

    #[test]
    fn a_render_session_carries_a_token_for_its_user_that_expires_within_minutes() {
        let iam = iam_service(86_400);
        let token = render_session(&iam, &context()).unwrap();

        let verified = iam.verify_token(&token).unwrap();
        assert_eq!(verified.user_id, Id::from_string("user-1"));
        assert_eq!(verified.org_id, Id::from_string("org-1"));
        assert_eq!(verified.scope, IamScope::Organization);

        let lifetime = crate::app::iam::test_support::token_lifetime_secs(&token);
        assert_eq!(lifetime, RENDER_SESSION_TTL_SECS);
    }

    #[test]
    fn a_render_token_never_outlives_the_configured_session() {
        let iam = iam_service(120);
        let token = render_session(&iam, &context()).unwrap();

        assert_eq!(
            crate::app::iam::test_support::token_lifetime_secs(&token),
            120
        );
    }
}
