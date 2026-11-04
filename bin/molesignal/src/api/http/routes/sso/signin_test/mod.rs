// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 测试登录：管理员对一个服务走一遍真实的登录，看身份源给出什么、会映射到哪个账号和角色。
//! 不建会话、不开户、不动绑定和角色，也不碰发起人当前的登录。
//!
//! - `POST /sso/providers/{id}/tests`：OIDC、OAuth 2.0、SAML 记下一条等待中的测试，回答要去的
//!   授权地址。发起人的浏览器走完身份源那边，回调还是登录那个地址（要在身份源注册的就是它）：
//!   `state` / `RelayState` 对上的是测试，就把结果记进这条测试，303 到结果页。
//! - `POST /sso/providers/{id}/tests/ldap`：带目录账号和密码当场核对。这是一次真的 bind，目录的
//!   锁定策略和登录限速（[`crate::app::iam::signin_limit`]）照算；密码不存、不进日志。
//! - `GET /sso/tests/{id}`：读一次测试，有人的邮箱和用户组，要管理权限。
//!
//! 停用的服务也能测，启用之前正该测。结果只对发起时的配置算数（[`config_hash`]）。
//!
//! 服务列表和登录要求看的是 [`standings`]。

mod failure;
mod preview;
mod standing;
mod view;

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};

pub(crate) use self::standing::has_passing_test;
pub(super) use self::standing::{TestStanding, standings};
use self::{
    failure::{Redactor, SETTINGS_CHANGED},
    view::{no_store, view_of},
};
use super::{
    ldap::to_flow_config, oauth, providers::ensure_same_org, provision::FederatedIdentity,
    random_token, saml,
};
use crate::{
    api::{AppState, http::signin_limit::SigninSource},
    app::iam::{
        IamContext,
        signin_limit::{SigninAccount, SigninError},
    },
    domain::iam::{IamScope, SsoProvider, SsoProviderConfig, SsoProviderKind, permission},
    infra::{
        persistence::repositories::audit_events::AuditEvent,
        sso::{
            LdapLoginFlow, SsoSigninTest, SsoSigninTestReport, SsoSigninTestStatus, SsoStateEntry,
            TestedFailure, TestedIdentity, config_hash,
        },
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

/// 前端的服务列表和结果页。
const PROVIDERS_PATH: &str = "/iam/sso";
const RESULT_PATH: &str = "/iam/sso/tests";

/// 一个服务每分钟最多发起几次：每次都留一条记录，走一趟身份源或目录。
const MAX_STARTS_PER_MINUTE: i64 = 10;

const MINUTE_MICROS: i64 = 60 * 1_000_000;

/// 用户组只存前面这么多，一次测试的结果不能无限大。
const MAX_GROUPS_KEPT: usize = 200;

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/sso/providers/{id}/tests", post(start))
        .route("/sso/providers/{id}/tests/ldap", post(start_ldap))
        .route("/sso/tests/{id}", get(read))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct StartResponse {
    id: String,
    authorization_url: String,
}

#[permission("org.sso.manage")]
async fn start(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<StartResponse>> {
    let provider = testable_provider(&state, &ctx, id).await?;
    let token = random_token();
    let nonce = random_token();

    let authorization_url = match provider.kind {
        SsoProviderKind::Oidc | SsoProviderKind::Oauth2 => {
            oauth::redirect_url(&provider, &token, &nonce)?.to_string()
        }
        SsoProviderKind::Saml => saml::redirect_url(&provider, &token)?,
        SsoProviderKind::Ldap => {
            return Err(Error::invalid(
                "a directory provider is tested with an account and a password",
            ));
        }
    };

    let test = new_test(&provider, &ctx.user_id);
    state.iam.sso_signin_tests.create(&test).await?;

    state
        .iam
        .sso_state_store
        .put(
            &token,
            &SsoStateEntry {
                provider_id: provider.id.0,
                nonce,
                next: None,
                test_id: Some(test.id.0.clone()),
            },
        )
        .await?;

    Ok(Json(StartResponse {
        id: test.id.0,
        authorization_url,
    }))
}

/// 没有 `Debug`：密码不能有机会进日志。
#[derive(Deserialize)]
struct DirectoryCredentials {
    username: String,
    password: String,
}

#[permission("org.sso.manage")]
async fn start_ldap(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    SigninSource(source): SigninSource,
    Path(id): Path<String>,
    Json(credentials): Json<DirectoryCredentials>,
) -> Result<Response, SigninError> {
    let provider = testable_provider(&state, &ctx, id).await?;

    let SsoProviderConfig::Ldap(config) = &provider.config else {
        return Err(
            Error::invalid(format!("provider {} is not a directory", provider.name)).into(),
        );
    };

    if credentials.username.trim().is_empty() || credentials.password.is_empty() {
        return Err(Error::invalid("enter a directory account and its password").into());
    }

    let flow = LdapLoginFlow::new(to_flow_config(config))?;

    // 这也是一次核对密码：和目录登录共用限速，不能拿测试绕开它。
    let account = SigninAccount::Directory {
        provider_id: &provider.id.0,
        username: &credentials.username,
    };

    let attempt = state.iam.signin_limiter.begin(&account, source).await?;

    let checked = flow
        .check(&credentials.username, &credentials.password)
        .await;

    attempt
        .settle(
            &checked
                .as_ref()
                .map_err(|failure| Error::from(failure.clone())),
        )
        .await;

    let redactor = Redactor::new(&provider, &[credentials.password.as_str()]);

    let report = match checked {
        Ok(user) => {
            let identity = FederatedIdentity {
                email: user.email,
                display_name: user.display_name,
                subject: user.subject,
                groups: user.groups,
                email_verified: None,
            };

            report_of(&state, &provider, &ctx.user_id, &identity, &redactor).await
        }
        Err(failure) => failed(failure::of_directory(&failure, &redactor)),
    };

    let mut test = new_test(&provider, &ctx.user_id);
    let now = TimestampMicros::now();
    test.status = status_of(&report);
    test.report = Some(report);
    test.finished_at = Some(now);

    state.iam.sso_signin_tests.create(&test).await?;
    record_outcome(&state, &provider, &test).await;

    let view = view_of(&state, &ctx, test, &provider).await?;

    Ok(no_store(view))
}

#[permission("org.sso.manage")]
async fn read(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Response> {
    let test = state.iam.sso_signin_tests.get(&Id::from_string(id)).await?;

    let provider = state.iam.sso_providers.get(&test.provider_id).await?;
    ensure_same_org(&provider, &ctx)?;

    let view = view_of(&state, &ctx, test, &provider).await?;

    Ok(no_store(view))
}

/// 这个人能测这个服务。令牌不算：测试要有人走一遍身份源，也不能给脚本一个试密码的口子。
async fn testable_provider(state: &AppState, ctx: &IamContext, id: String) -> Result<SsoProvider> {
    if ctx.scope == IamScope::ApiToken {
        return Err(Error::forbidden("sign in to test a provider"));
    }

    let provider = state.iam.sso_providers.get(&Id::from_string(id)).await?;
    ensure_same_org(&provider, ctx)?;

    let since = TimestampMicros(TimestampMicros::now().0.saturating_sub(MINUTE_MICROS));

    let recent = state
        .iam
        .sso_signin_tests
        .started_since(&provider.id, since)
        .await?;

    if recent >= MAX_STARTS_PER_MINUTE {
        return Err(Error::resource_exhausted(
            "too many test sign-ins for this provider; wait a minute",
        ));
    }

    Ok(provider)
}

fn new_test(provider: &SsoProvider, started_by: &Id) -> SsoSigninTest {
    SsoSigninTest {
        id: Id::new(),
        provider_id: provider.id.clone(),
        started_by: started_by.clone(),
        config_hash: config_hash(provider),
        status: SsoSigninTestStatus::Pending,
        report: None,
        started_at: TimestampMicros::now(),
        finished_at: None,
    }
}

/// 回调等到的一次测试。
pub(super) struct Run {
    test: SsoSigninTest,
    provider: SsoProvider,
}

impl Run {
    /// 取回调要记结果的测试。这条测试或它的服务已经删了时是 `None`，由 [`Self::lost`] 回答。
    pub(super) async fn open(state: &AppState, test_id: &str) -> Option<Self> {
        let found = async {
            let test = state
                .iam
                .sso_signin_tests
                .get(&Id::from_string(test_id))
                .await?;

            let provider = state.iam.sso_providers.get(&test.provider_id).await?;

            Ok::<_, Error>(Self { test, provider })
        };

        match found.await {
            Ok(run) => Some(run),
            Err(error) => {
                tracing::warn!(error = %error, "sso sign-in test could not be continued");
                None
            }
        }
    }

    /// 没有测试可记时，回到服务列表。
    pub(super) fn lost() -> Response {
        Redirect::to(PROVIDERS_PATH).into_response()
    }

    pub(super) fn provider(&self) -> &SsoProvider {
        &self.provider
    }

    /// 发起之后配置改了：这次一半走的旧配置，结果说明不了当前的配置。
    pub(super) fn check_settings(&self) -> Result<()> {
        if config_hash(&self.provider) == self.test.config_hash {
            Ok(())
        } else {
            Err(Error::invalid(SETTINGS_CHANGED))
        }
    }

    /// 记下结果，把浏览器送到结果页。`seen` 是这次回调里不能露出来的值，如授权码。
    pub(super) async fn conclude(
        self,
        state: &AppState,
        outcome: Result<FederatedIdentity>,
        seen: &[String],
    ) -> Response {
        let Self { mut test, provider } = self;
        let seen = seen.iter().map(String::as_str).collect::<Vec<_>>();
        let redactor = Redactor::new(&provider, &seen);

        let report = match outcome {
            Ok(identity) => {
                report_of(state, &provider, &test.started_by, &identity, &redactor).await
            }
            Err(error) => failed(failure::of_error(&error, &redactor)),
        };

        let now = TimestampMicros::now();
        test.status = status_of(&report);
        test.finished_at = Some(now);

        let saved = state
            .iam
            .sso_signin_tests
            .finish(&test.id, test.status, &report, now)
            .await;

        if let Err(error) = saved {
            tracing::warn!(error = %error, "sso sign-in test result could not be saved");
        }

        test.report = Some(report);
        record_outcome(state, &provider, &test).await;

        Redirect::to(&format!("{RESULT_PATH}/{}", test.id.0)).into_response()
    }
}

fn status_of(report: &SsoSigninTestReport) -> SsoSigninTestStatus {
    if report.failure.is_some() {
        SsoSigninTestStatus::Failed
    } else {
        SsoSigninTestStatus::Passed
    }
}

fn failed(failure: TestedFailure) -> SsoSigninTestReport {
    SsoSigninTestReport {
        failure: Some(failure),
        ..SsoSigninTestReport::default()
    }
}

/// 身份拿到了：看这次登录会落到哪儿。规则不放行也算没通过，但身份照样给管理员看。
async fn report_of(
    state: &AppState,
    provider: &SsoProvider,
    tester: &Id,
    identity: &FederatedIdentity,
    redactor: &Redactor,
) -> SsoSigninTestReport {
    let tested = tested_identity(identity);

    match preview::preview(state, provider, identity, tester).await {
        Ok(preview) => SsoSigninTestReport {
            identity: Some(tested),
            mapping: Some(preview.mapping),
            failure: None,
            notes: preview.notes,
        },
        Err(error) => SsoSigninTestReport {
            identity: Some(tested),
            mapping: None,
            failure: Some(failure::of_error(&error, redactor)),
            notes: Vec::new(),
        },
    }
}

fn tested_identity(identity: &FederatedIdentity) -> TestedIdentity {
    TestedIdentity {
        email: identity.email.trim().to_owned(),
        display_name: identity.display_name.trim().to_owned(),
        subject: identity.subject.clone(),
        groups: identity
            .groups
            .iter()
            .take(MAX_GROUPS_KEPT)
            .cloned()
            .collect(),
        groups_total: identity.groups.len(),
        email_verified: identity.email_verified,
    }
}

/// 日志和审计只留谁测了哪个服务、结果和失败类别：身份和身份源的原话都不留。
async fn record_outcome(state: &AppState, provider: &SsoProvider, test: &SsoSigninTest) {
    let failure = test
        .report
        .as_ref()
        .and_then(|report| report.failure.as_ref())
        .map(|failure| failure.code.as_str());

    tracing::info!(
        provider_id = %provider.id.0,
        kind = provider.kind.as_str(),
        status = test.status.as_str(),
        failure = failure.unwrap_or_default(),
        "sso sign-in test finished"
    );

    let recorded = state
        .iam
        .audit_events
        .record(AuditEvent {
            id: Id::new(),
            org_id: provider.org_id.clone(),
            actor_kind: "user".into(),
            actor_id: test.started_by.0.clone(),
            action: "org.sso_provider.test".into(),
            target_kind: Some("sso_provider".into()),
            target_id: Some(provider.id.0.clone()),
            ip: None,
            user_agent: None,
            payload: serde_json::json!({
                "testId": test.id.0,
                "name": provider.name,
                "kind": provider.kind.as_str(),
                "status": test.status.as_str(),
                "failure": failure,
            }),
            ts: test.finished_at.unwrap_or_else(TimestampMicros::now),
        })
        .await;

    if let Err(error) = recorded {
        tracing::warn!(error = %error, "sso sign-in test audit event could not be recorded");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_list_of_groups_is_cut_but_still_counted() {
        let identity = FederatedIdentity {
            email: " alice@example.com ".into(),
            display_name: " Alice ".into(),
            subject: "u-1".into(),
            groups: (0..MAX_GROUPS_KEPT + 5)
                .map(|n| format!("group-{n}"))
                .collect(),
            email_verified: Some(true),
        };

        let tested = tested_identity(&identity);

        assert_eq!(tested.email, "alice@example.com");
        assert_eq!(tested.display_name, "Alice");
        assert_eq!(tested.groups.len(), MAX_GROUPS_KEPT);
        assert_eq!(tested.groups_total, MAX_GROUPS_KEPT + 5);
    }
}
