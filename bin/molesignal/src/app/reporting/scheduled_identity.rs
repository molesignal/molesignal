// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 定时投递以谁的身份渲染：最后一次保存这份报表的登录用户。
//!
//! 谁最后决定了「渲染什么、发给谁」，报表就用谁的权限取数：有权限改报表的人把收件地址改成
//! 自己的，报表随之改用这个人的权限，拿不到上一位保存者才看得到的内容。每次运行重新核对这位
//! 用户：账号还能登录、还在组织里、还读得到报表引用的仪表盘或保存视图。任何一条不成立，这次
//! 运行就失败，原因写进投递记录；有 `reports.schedule` 的人重新保存即可接管。

use std::sync::Arc;

use async_trait::async_trait;

use super::render_session::{ReportRenderSession, render_session};
use crate::{
    app::iam::{IamAccessRequest, IamAccessService, IamContext, IamService, IamTarget},
    domain::iam::{IamScope, User},
    infra::persistence::repositories::scheduled_reports::ScheduledReport,
    shared::{Error, Result, ids::Id},
};

/// 报表以谁的身份运行：保存它的登录用户。
///
/// API Token 只代表它绑定的那个角色，以签发者本人的身份去渲染会越过这个角色，所以由 API
/// Token 保存的报表没有可用身份。
pub fn run_as_user(saver: &IamContext) -> Option<Id> {
    (saver.scope == IamScope::Organization).then(|| saver.user_id.clone())
}

/// 渲染身份要向 IAM 问的事。
#[async_trait]
trait RenderIdentityDirectory: Send + Sync {
    /// 用户本人；不存在时返回 [`Error::NotFound`]。
    async fn user(&self, user_id: &Id) -> Result<User>;

    /// 用户此刻在组织里的身份，角色和权限已解析。账号或组织被停用、用户不再是成员时返回
    /// [`Error::Unauthorized`] 或 [`Error::Forbidden`]，文本说明是哪一种。
    async fn access(&self, user_id: &Id, org_id: &Id) -> Result<IamContext>;

    /// `context` 能否对组织里的这个资源执行 `permission`。
    async fn allows(
        &self,
        context: &IamContext,
        permission: &str,
        resource_type: &str,
        resource_id: &str,
    ) -> Result<bool>;

    /// 给 `context` 签发一次渲染用的登录态。
    fn session(&self, context: &IamContext) -> Result<String>;
}

/// 定时投递的渲染身份：见模块文档。
pub struct ScheduledReportIdentity {
    directory: Arc<dyn RenderIdentityDirectory>,
}

impl ScheduledReportIdentity {
    pub fn new(iam: Arc<IamService>, access: Arc<IamAccessService>) -> Self {
        Self::with_directory(Arc::new(IamDirectory { iam, access }))
    }

    fn with_directory(directory: Arc<dyn RenderIdentityDirectory>) -> Self {
        Self { directory }
    }

    /// 报表引用的仪表盘或保存视图，这位用户还读得到。
    async fn ensure_reads_source(
        &self,
        context: &IamContext,
        who: &str,
        report: &ScheduledReport,
    ) -> Result<()> {
        let (permission, resource_type, source) =
            match (&report.dashboard_id, &report.saved_view_id) {
                (Some(id), _) => ("dashboards.read", "dashboard", id),
                (None, Some(id)) => ("saved_views.read", "saved_view", id),
                (None, None) => {
                    return Err(Error::invalid(
                        "report must reference a dashboard or saved view",
                    ));
                }
            };

        let readable = self
            .directory
            .allows(context, permission, resource_type, source.as_str())
            .await?;

        if !readable {
            return Err(unusable(
                who,
                &format!(
                    "can no longer read the {} ({permission})",
                    resource_type.replace('_', " ")
                ),
            ));
        }

        // 保存视图的报表就是它的查询结果；仪表盘里不取数的面板不需要这项权限。
        if report.dashboard_id.is_none() && !context.has_permission("streams.query") {
            return Err(unusable(who, "can no longer query streams (streams.query)"));
        }

        Ok(())
    }
}

#[async_trait]
impl ReportRenderSession for ScheduledReportIdentity {
    async fn issue(&self, report: &ScheduledReport) -> Result<String> {
        let Some(user_id) = &report.run_as_user_id else {
            return Err(Error::forbidden(NO_USER));
        };

        let user = match self.directory.user(user_id).await {
            Ok(user) => user,
            Err(Error::NotFound(_)) => {
                return Err(unusable(&format!("user {}", user_id.0), "no longer exists"));
            }
            Err(error) => return Err(error),
        };

        let context = match self.directory.access(&user.id, &report.org_id).await {
            Ok(context) => context,
            Err(Error::Unauthorized(reason) | Error::Forbidden(reason)) => {
                return Err(unusable(
                    &user.email,
                    &format!("can no longer sign in to this organization ({reason})"),
                ));
            }
            Err(error) => return Err(error),
        };

        self.ensure_reads_source(&context, &user.email, report)
            .await?;

        self.directory.session(&context)
    }
}

const NO_USER: &str = "the report has no user to run as: it was last saved with an API token. \
                       Save it again while signed in to run it with that user's access";

/// 报表记着的那位用户这次用不了。
fn unusable(who: &str, why: &str) -> Error {
    Error::forbidden(format!(
        "the report runs as {who}, who {why}. Save the report again to run it as another user"
    ))
}

/// 生产实现：读 IAM 的用户、成员关系和授权判定。
struct IamDirectory {
    iam: Arc<IamService>,
    access: Arc<IamAccessService>,
}

#[async_trait]
impl RenderIdentityDirectory for IamDirectory {
    async fn user(&self, user_id: &Id) -> Result<User> {
        self.iam.current_user(user_id).await
    }

    async fn access(&self, user_id: &Id, org_id: &Id) -> Result<IamContext> {
        // 与请求进来时的鉴权同一套核对：账号状态、组织状态，再解析成员关系和权限。
        self.iam.ensure_user_access(user_id).await?;
        self.iam.ensure_organization_access(org_id).await?;

        let mut context = IamContext {
            user_id: user_id.clone(),
            org_id: org_id.clone(),
            display_role: String::new(),
            roles: Vec::new(),
            credential_role_id: None,
            credential_application_id: None,
            credential_service_account_id: None,
            scope: IamScope::Organization,
            permissions: Default::default(),
            policy_version: 0,
        };

        self.access.enrich_context(&mut context).await?;
        Ok(context)
    }

    async fn allows(
        &self,
        context: &IamContext,
        permission: &str,
        resource_type: &str,
        resource_id: &str,
    ) -> Result<bool> {
        let request = IamAccessRequest {
            permission: permission.to_owned(),
            target: Some(IamTarget {
                organization_id: Some(context.org_id.0.clone()),
                resource_type: resource_type.to_owned(),
                resource_id: resource_id.to_owned(),
                container_type: None,
                container_id: None,
            }),
            attributes: Default::default(),
        };

        Ok(self.access.evaluate(context, &request).await?.allowed)
    }

    fn session(&self, context: &IamContext) -> Result<String> {
        render_session(&self.iam, context)
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeSet, sync::Mutex};

    use serde_json::json;

    use super::*;
    use crate::{domain::iam::UserStatus, shared::time::TimestampMicros};

    /// IAM 对「这位用户此刻在组织里的身份」的回答。
    enum Access {
        /// 可以登录，持有这些权限。
        Granted(Vec<&'static str>),
        Unauthorized(&'static str),
        Forbidden(&'static str),
        /// IAM 自己出了故障，与这位用户无关。
        Unreachable(&'static str),
    }

    /// IAM 对一位用户的回答，以及被问过哪些资源。
    struct Directory {
        user: Option<User>,
        access: Access,
        readable: bool,
        asked: Mutex<Vec<(String, String, String)>>,
    }

    impl Directory {
        /// Ada 在组织里，持有 `permissions`，读得到被问到的资源。
        fn with_ada(permissions: &[&'static str]) -> Self {
            Self {
                user: Some(User {
                    id: Id::from_string("u-ada"),
                    email: "ada@example.com".into(),
                    display_name: "Ada".into(),
                    avatar_url: None,
                    bio: String::new(),
                    password_hash: String::new(),
                    disabled: false,
                    status: UserStatus::Active,
                    created_at: TimestampMicros(0),
                }),
                access: Access::Granted(permissions.to_vec()),
                readable: true,
                asked: Mutex::new(Vec::new()),
            }
        }

        fn asked(&self) -> Vec<(String, String, String)> {
            self.asked.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl RenderIdentityDirectory for Directory {
        async fn user(&self, user_id: &Id) -> Result<User> {
            self.user
                .clone()
                .filter(|user| &user.id == user_id)
                .ok_or_else(|| Error::not_found("user"))
        }

        async fn access(&self, user_id: &Id, org_id: &Id) -> Result<IamContext> {
            let permissions = match &self.access {
                Access::Granted(permissions) => permissions,
                Access::Unauthorized(reason) => return Err(Error::unauthorized(*reason)),
                Access::Forbidden(reason) => return Err(Error::forbidden(*reason)),
                Access::Unreachable(reason) => return Err(Error::internal(*reason)),
            };

            Ok(IamContext {
                user_id: user_id.clone(),
                org_id: org_id.clone(),
                display_role: "Editor".into(),
                roles: Vec::new(),
                credential_role_id: None,
                credential_application_id: None,
                credential_service_account_id: None,
                scope: IamScope::Organization,
                permissions: permissions
                    .iter()
                    .map(|key| (*key).to_owned())
                    .collect::<BTreeSet<_>>(),
                policy_version: 1,
            })
        }

        async fn allows(
            &self,
            _: &IamContext,
            permission: &str,
            resource_type: &str,
            resource_id: &str,
        ) -> Result<bool> {
            self.asked.lock().unwrap().push((
                permission.to_owned(),
                resource_type.to_owned(),
                resource_id.to_owned(),
            ));

            Ok(self.readable)
        }

        fn session(&self, context: &IamContext) -> Result<String> {
            Ok(format!(
                "session-for-{}@{}",
                context.user_id.0, context.org_id.0
            ))
        }
    }

    fn report(run_as: Option<&str>) -> ScheduledReport {
        ScheduledReport {
            id: Id::from_string("r1"),
            org_id: Id::from_string("org-1"),
            name: "weekly".into(),
            dashboard_id: Some(Id::from_string("d1")),
            saved_view_id: None,
            cron: "every:7d".into(),
            recipients: Vec::new(),
            format: "pdf".into(),
            time_range_json: json!({ "preset": "previous-7-days" }),
            enabled: true,
            run_as_user_id: run_as.map(Id::from_string),
            last_run_at: None,
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(0),
        }
    }

    fn saved_view_report() -> ScheduledReport {
        ScheduledReport {
            dashboard_id: None,
            saved_view_id: Some(Id::from_string("v1")),
            ..report(Some("u-ada"))
        }
    }

    async fn issue(
        directory: Directory,
        report: &ScheduledReport,
    ) -> (Result<String>, Arc<Directory>) {
        let directory = Arc::new(directory);
        let identity = ScheduledReportIdentity::with_directory(directory.clone());

        (identity.issue(report).await, directory)
    }

    fn refusal(result: Result<String>) -> String {
        match result {
            Err(Error::Forbidden(reason)) => reason,
            other => panic!("expected the identity to be refused, got {other:?}"),
        }
    }

    fn context(scope: IamScope) -> IamContext {
        IamContext {
            user_id: Id::from_string("u-ada"),
            org_id: Id::from_string("org-1"),
            display_role: String::new(),
            roles: Vec::new(),
            credential_role_id: None,
            credential_application_id: None,
            credential_service_account_id: None,
            scope,
            permissions: BTreeSet::new(),
            policy_version: 0,
        }
    }

    #[test]
    fn a_report_runs_as_the_signed_in_user_who_saves_it() {
        assert_eq!(
            run_as_user(&context(IamScope::Organization)),
            Some(Id::from_string("u-ada"))
        );
    }

    #[test]
    fn a_report_saved_with_an_api_token_has_no_user_to_run_as() {
        // 以签发者本人的身份渲染会越过 token 绑定的角色。
        assert_eq!(run_as_user(&context(IamScope::ApiToken)), None);
        assert_eq!(run_as_user(&context(IamScope::System)), None);
    }

    #[tokio::test]
    async fn a_dashboard_report_is_rendered_as_the_user_who_last_saved_it() {
        let (session, directory) = issue(Directory::with_ada(&[]), &report(Some("u-ada"))).await;

        assert_eq!(session.unwrap(), "session-for-u-ada@org-1");
        // 核对的是报表引用的那一个仪表盘。
        assert_eq!(
            directory.asked(),
            vec![("dashboards.read".into(), "dashboard".into(), "d1".into())]
        );
    }

    #[tokio::test]
    async fn a_saved_view_report_needs_the_view_and_the_right_to_query() {
        let (session, directory) = issue(
            Directory::with_ada(&["streams.query"]),
            &saved_view_report(),
        )
        .await;

        assert_eq!(session.unwrap(), "session-for-u-ada@org-1");
        assert_eq!(
            directory.asked(),
            vec![("saved_views.read".into(), "saved_view".into(), "v1".into())]
        );

        let (session, _) = issue(Directory::with_ada(&[]), &saved_view_report()).await;

        assert_eq!(
            refusal(session),
            "the report runs as ada@example.com, who can no longer query streams \
             (streams.query). Save the report again to run it as another user"
        );
    }

    #[tokio::test]
    async fn a_report_saved_with_an_api_token_is_refused_without_asking_iam() {
        let (session, directory) = issue(Directory::with_ada(&[]), &report(None)).await;

        assert_eq!(refusal(session), NO_USER);
        assert!(directory.asked().is_empty());
    }

    #[tokio::test]
    async fn a_user_who_no_longer_exists_is_named_by_id() {
        let directory = Directory {
            user: None,
            ..Directory::with_ada(&[])
        };
        let (session, _) = issue(directory, &report(Some("u-ada"))).await;

        assert_eq!(
            refusal(session),
            "the report runs as user u-ada, who no longer exists. Save the report again to run \
             it as another user"
        );
    }

    #[tokio::test]
    async fn a_user_who_cannot_sign_in_is_named_with_the_reason() {
        let refusals = [
            (Access::Unauthorized("user disabled"), "user disabled"),
            (
                Access::Forbidden("active organization membership required"),
                "active organization membership required",
            ),
            (
                Access::Forbidden("organization is disabled by a platform administrator"),
                "organization is disabled by a platform administrator",
            ),
        ];

        for (access, reason) in refusals {
            let directory = Directory {
                access,
                ..Directory::with_ada(&[])
            };
            let (session, directory) = issue(directory, &report(Some("u-ada"))).await;

            assert_eq!(
                refusal(session),
                format!(
                    "the report runs as ada@example.com, who can no longer sign in to this \
                     organization ({reason}). Save the report again to run it as another user"
                )
            );
            // 登不进来就不必再问读不读得到。
            assert!(directory.asked().is_empty());
        }
    }

    #[tokio::test]
    async fn a_user_who_lost_the_source_is_named_with_what_was_lost() {
        let unreadable = || Directory {
            readable: false,
            ..Directory::with_ada(&["streams.query"])
        };

        let (session, _) = issue(unreadable(), &report(Some("u-ada"))).await;
        assert_eq!(
            refusal(session),
            "the report runs as ada@example.com, who can no longer read the dashboard \
             (dashboards.read). Save the report again to run it as another user"
        );

        let (session, _) = issue(unreadable(), &saved_view_report()).await;
        assert_eq!(
            refusal(session),
            "the report runs as ada@example.com, who can no longer read the saved view \
             (saved_views.read). Save the report again to run it as another user"
        );
    }

    #[tokio::test]
    async fn a_failure_to_reach_iam_is_not_reported_as_a_missing_identity() {
        let directory = Directory {
            access: Access::Unreachable("pool timed out"),
            ..Directory::with_ada(&[])
        };
        let (session, _) = issue(directory, &report(Some("u-ada"))).await;

        assert!(matches!(session, Err(Error::Internal(reason)) if reason == "pool timed out"));
    }
}
