// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 结果页读到的一次测试。结果里有人的邮箱和用户组，回答不留在缓存里。

use axum::{
    Json,
    http::header::CACHE_CONTROL,
    response::{IntoResponse, Response},
};
use serde::Serialize;

use crate::{
    api::AppState,
    app::iam::IamContext,
    domain::iam::{SsoPlatform, SsoProvider, SsoProviderKind},
    infra::sso::{SsoSigninTest, SsoSigninTestReport, SsoSigninTestStatus, config_hash},
    shared::{Result, time::TimestampMicros},
};

/// 等身份源回调的时间，和 `state` 的有效期一样（`bootstrap::iam` 的 `SsoStateStore`）。过了还没有
/// 结果，多半是身份源那边的错误页面停住了。
const PENDING_TTL_MICROS: i64 = 600 * 1_000_000;

/// 一次测试，给结果页看。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct TestView {
    id: String,
    provider_id: String,
    provider_name: String,
    kind: SsoProviderKind,
    platform: SsoPlatform,
    /// `pending`、`passed`、`failed`，或等太久没回来的 `expired`。
    status: &'static str,
    /// 结果对应的还是当前配置。
    current: bool,
    started_by: String,
    started_by_you: bool,
    started_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    finished_at: Option<i64>,
    #[serde(flatten)]
    report: SsoSigninTestReport,
}

pub(super) async fn view_of(
    state: &AppState,
    ctx: &IamContext,
    test: SsoSigninTest,
    provider: &SsoProvider,
) -> Result<TestView> {
    let starter = state.iam.service.users.get(&test.started_by).await?;
    let status = shown_status(test.status, test.started_at, TimestampMicros::now());

    Ok(TestView {
        current: config_hash(provider) == test.config_hash,
        id: test.id.0,
        provider_id: provider.id.0.clone(),
        provider_name: provider.name.clone(),
        kind: provider.kind,
        platform: provider.platform,
        status,
        started_by: starter.display_name,
        started_by_you: test.started_by == ctx.user_id,
        started_at: test.started_at.0,
        finished_at: test.finished_at.map(|at| at.0),
        report: test.report.unwrap_or_default(),
    })
}

fn shown_status(
    status: SsoSigninTestStatus,
    started_at: TimestampMicros,
    now: TimestampMicros,
) -> &'static str {
    let waited = now.0.saturating_sub(started_at.0);

    if status == SsoSigninTestStatus::Pending && waited > PENDING_TTL_MICROS {
        "expired"
    } else {
        status.as_str()
    }
}

pub(super) fn no_store(view: TestView) -> Response {
    ([(CACHE_CONTROL, "no-store")], Json(view)).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::sso::TestedFailure;

    #[test]
    fn a_test_that_never_came_back_reads_expired() {
        let started = TimestampMicros(1_000);
        let soon = TimestampMicros(1_000 + PENDING_TTL_MICROS);
        let later = TimestampMicros(1_001 + PENDING_TTL_MICROS);

        assert_eq!(
            shown_status(SsoSigninTestStatus::Pending, started, soon),
            "pending"
        );
        assert_eq!(
            shown_status(SsoSigninTestStatus::Pending, started, later),
            "expired"
        );
        assert_eq!(
            shown_status(SsoSigninTestStatus::Passed, started, later),
            "passed"
        );
    }

    #[test]
    fn the_answer_for_a_browser_has_flat_report_fields() {
        let report = SsoSigninTestReport {
            failure: Some(TestedFailure {
                code: "token_exchange".into(),
                detail: None,
            }),
            ..SsoSigninTestReport::default()
        };

        let view = TestView {
            id: "t-1".into(),
            provider_id: "p-1".into(),
            provider_name: "Okta".into(),
            kind: SsoProviderKind::Oidc,
            platform: SsoPlatform::Okta,
            status: "failed",
            current: true,
            started_by: "Dev".into(),
            started_by_you: true,
            started_at: 1,
            finished_at: Some(2),
            report,
        };

        let json = serde_json::to_value(view).expect("view");

        assert_eq!(json["failure"]["code"], "token_exchange");
        assert_eq!(json["kind"], "oidc");
        assert!(json.get("identity").is_none());
    }
}
