// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 服务当前配置的测试情况，给服务列表和登录要求看：当前配置最近一次结果说了算；有结果但配置后来
//! 改过的算作废。

use std::collections::HashMap;

use serde::Serialize;

use crate::{
    api::AppState,
    domain::iam::{SsoProvider, SsoProviderKind},
    infra::sso::{SsoSigninTestStatus, SsoSigninTestSummary, config_hash},
    shared::{Result, ids::Id},
};

/// 服务当前配置的测试情况。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(in super::super) enum TestStandingStatus {
    /// 还没有结果。
    Untested,
    Passed,
    Failed,
    /// 有结果，但配置后来改过，作废了。
    Stale,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(in super::super) struct TestStanding {
    pub status: TestStandingStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub test_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<i64>,
}

/// `finished` 是这个服务有结果的测试，新的在前。当前配置最近一次结果说了算。
fn standing_of(current_hash: &str, finished: &[&SsoSigninTestSummary]) -> TestStanding {
    let latest = finished
        .iter()
        .find(|test| test.config_hash == current_hash)
        .or_else(|| finished.first());

    let Some(latest) = latest else {
        return TestStanding {
            status: TestStandingStatus::Untested,
            test_id: None,
            finished_at: None,
        };
    };

    let status = if latest.config_hash != current_hash {
        TestStandingStatus::Stale
    } else if latest.status == SsoSigninTestStatus::Passed {
        TestStandingStatus::Passed
    } else {
        TestStandingStatus::Failed
    };

    TestStanding {
        status,
        test_id: Some(latest.id.0.clone()),
        finished_at: latest.finished_at.map(|at| at.0),
    }
}

/// 这些服务当前配置的测试情况，按服务 id。
pub(in super::super) async fn standings(
    state: &AppState,
    providers: &[SsoProvider],
) -> Result<HashMap<String, TestStanding>> {
    let ids = providers
        .iter()
        .map(|provider| provider.id.clone())
        .collect::<Vec<_>>();

    let finished = state.iam.sso_signin_tests.finished_of(&ids).await?;

    Ok(providers
        .iter()
        .map(|provider| {
            let own = finished
                .iter()
                .filter(|test| test.provider_id == provider.id)
                .collect::<Vec<_>>();

            (
                provider.id.0.clone(),
                standing_of(&config_hash(provider), &own),
            )
        })
        .collect())
}

/// 组织有启用的单点登录服务通过了测试（当前配置）。LDAP 不算单点登录。
pub(crate) async fn has_passing_test(state: &AppState, org_id: &Id) -> Result<bool> {
    let providers = state
        .iam
        .sso_providers
        .list_enabled(org_id)
        .await?
        .into_iter()
        .filter(|provider| provider.kind != SsoProviderKind::Ldap)
        .collect::<Vec<_>>();

    let standings = standings(state, &providers).await?;

    Ok(standings
        .values()
        .any(|standing| standing.status == TestStandingStatus::Passed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::time::TimestampMicros;

    fn finished(id: &str, hash: &str, status: SsoSigninTestStatus) -> SsoSigninTestSummary {
        SsoSigninTestSummary {
            id: Id::from_string(id),
            provider_id: Id::from_string("p-1"),
            config_hash: hash.to_owned(),
            status,
            finished_at: Some(TimestampMicros(1_000)),
        }
    }

    #[test]
    fn no_result_means_untested() {
        let standing = standing_of("now", &[]);

        assert_eq!(standing.status, TestStandingStatus::Untested);
        assert_eq!(standing.test_id, None);
    }

    #[test]
    fn the_latest_result_of_the_current_settings_decides() {
        let failed = finished("t-2", "now", SsoSigninTestStatus::Failed);
        let passed = finished("t-1", "now", SsoSigninTestStatus::Passed);

        let standing = standing_of("now", &[&failed, &passed]);
        assert_eq!(standing.status, TestStandingStatus::Failed);
        assert_eq!(standing.test_id.as_deref(), Some("t-2"));

        let standing = standing_of("now", &[&passed, &failed]);
        assert_eq!(standing.status, TestStandingStatus::Passed);
        assert_eq!(standing.test_id.as_deref(), Some("t-1"));
    }

    #[test]
    fn a_pass_on_earlier_settings_is_void() {
        let old_pass = finished("t-1", "before", SsoSigninTestStatus::Passed);

        let standing = standing_of("now", &[&old_pass]);
        assert_eq!(standing.status, TestStandingStatus::Stale);
        assert_eq!(standing.test_id.as_deref(), Some("t-1"));
    }

    #[test]
    fn a_result_on_the_current_settings_beats_a_newer_one_on_other_settings() {
        let other = finished("t-2", "other", SsoSigninTestStatus::Failed);
        let current = finished("t-1", "now", SsoSigninTestStatus::Passed);

        let standing = standing_of("now", &[&other, &current]);
        assert_eq!(standing.status, TestStandingStatus::Passed);
        assert_eq!(standing.test_id.as_deref(), Some("t-1"));
    }
}
