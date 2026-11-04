// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 测试登录的记录（`sso_signin_tests`）。
//!
//! 管理员对一个服务走一遍真实的登录，这里记下身份源给出的身份、它会映射到的账号和角色，或失败
//! 原因，没有令牌、断言和密码。每条记录带发起时服务配置的摘要（[`config_hash`]）：配置一改，旧结果
//! 就对不上，不用哪条写配置的路径记得去作废它。

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row, types::Json};

use crate::{
    domain::iam::SsoProvider,
    infra::persistence::sqlx_err,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

/// 每个服务留最近多少条：结果里有人的邮箱和用户组，不长期存。
const KEEP_PER_PROVIDER: i64 = 20;

const COLS: &str =
    "id, provider_id, started_by, config_hash, status, report, started_at, finished_at";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SsoSigninTestStatus {
    /// 等身份源回调。
    Pending,
    Passed,
    Failed,
}

impl SsoSigninTestStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Passed => "passed",
            Self::Failed => "failed",
        }
    }

    fn parse(value: &str) -> Result<Self> {
        match value {
            "pending" => Ok(Self::Pending),
            "passed" => Ok(Self::Passed),
            "failed" => Ok(Self::Failed),
            other => Err(Error::internal(format!(
                "unknown sso_signin_tests.status value: {other}"
            ))),
        }
    }
}

/// 身份源给出的身份。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestedIdentity {
    pub email: String,
    pub display_name: String,
    pub subject: String,
    /// 身份源给出的用户组，太多时只存前面一部分。
    pub groups: Vec<String>,
    /// 身份源给出的用户组总数。
    pub groups_total: usize,
    #[serde(default)]
    pub email_verified: Option<bool>,
}

/// 这个身份落到哪种账号。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TestedAccount {
    /// 身份已绑定账号，直接登录。
    Bound,
    /// 邮箱已有账号，第一次登录时本人确认后才关联。
    Link,
    /// 没有账号，第一次登录时新建。
    Create,
}

/// 角色从哪儿来。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TestedRoleSource {
    GroupMapping,
    DefaultRole,
    /// 没有映射到角色的新成员：自助注册的角色。
    Signup,
    /// 没有映射到角色的老成员：角色不动。
    Unchanged,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TestedRole {
    pub id: String,
    pub name: String,
}

/// 这个身份会映射到什么。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestedMapping {
    pub account: TestedAccount,
    /// 账号的邮箱；不是本组织成员的账号只露首字母和域名。
    pub account_email: String,
    /// 本组织成员的名字。
    #[serde(default)]
    pub account_name: Option<String>,
    /// 这个账号就是发起测试的人。
    pub is_tester: bool,
    /// 账号已是本组织成员。
    pub member: bool,
    pub current_roles: Vec<TestedRole>,
    /// 登录之后的角色。
    pub roles: Vec<TestedRole>,
    pub role_source: TestedRoleSource,
    /// 命中角色映射的用户组。
    pub matched_groups: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TestedFailure {
    /// 失败的类别，前端按它给说明和处理建议。
    pub code: String,
    /// 服务端或身份源的原话，去掉了敏感值。
    #[serde(default)]
    pub detail: Option<String>,
}

/// 一次测试的结果。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SsoSigninTestReport {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<TestedIdentity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mapping: Option<TestedMapping>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<TestedFailure>,
    /// 通过了也值得看一眼的事，前端按它给说明。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct SsoSigninTest {
    pub id: Id,
    pub provider_id: Id,
    pub started_by: Id,
    pub config_hash: String,
    pub status: SsoSigninTestStatus,
    pub report: Option<SsoSigninTestReport>,
    pub started_at: TimestampMicros,
    pub finished_at: Option<TimestampMicros>,
}

/// 服务列表要看的部分。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SsoSigninTestSummary {
    pub id: Id,
    pub provider_id: Id,
    pub config_hash: String,
    pub status: SsoSigninTestStatus,
    pub finished_at: Option<TimestampMicros>,
}

/// 决定这个服务怎么登录的一切：接入类型、平台读法和整份配置，含密钥。摘要只用来比较，不离开
/// 服务端。改名和启停不影响登录，不算在内。
pub fn config_hash(provider: &SsoProvider) -> String {
    let config = serde_json::to_vec(&provider.config).expect("sso provider config serializes");
    let mut hasher = Sha256::new();

    hasher.update(provider.kind.as_str());
    hasher.update([0]);
    hasher.update(provider.platform.as_str());
    hasher.update([0]);
    hasher.update(&config);

    hex::encode(hasher.finalize())
}

#[async_trait]
pub trait SsoSigninTestRepository: Send + Sync {
    /// 存下一条测试，只留这个服务最近的一批。
    async fn create(&self, test: &SsoSigninTest) -> Result<()>;
    async fn get(&self, id: &Id) -> Result<SsoSigninTest>;
    async fn finish(
        &self,
        id: &Id,
        status: SsoSigninTestStatus,
        report: &SsoSigninTestReport,
        at: TimestampMicros,
    ) -> Result<()>;
    /// 这个服务从 `since` 起发起了多少次，用于限速。
    async fn started_since(&self, provider_id: &Id, since: TimestampMicros) -> Result<i64>;
    /// 这些服务有结果的测试，新的在前。
    async fn finished_of(&self, provider_ids: &[Id]) -> Result<Vec<SsoSigninTestSummary>>;
}

pub struct PgSsoSigninTestRepository {
    pool: PgPool,
}

impl PgSsoSigninTestRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn row_to_test(row: sqlx::postgres::PgRow) -> Result<SsoSigninTest> {
    let status: String = row.try_get("status").map_err(sqlx_err)?;
    let report: Option<Json<SsoSigninTestReport>> = row.try_get("report").map_err(sqlx_err)?;

    Ok(SsoSigninTest {
        id: Id::from_string(row.try_get::<String, _>("id").map_err(sqlx_err)?),
        provider_id: Id::from_string(row.try_get::<String, _>("provider_id").map_err(sqlx_err)?),
        started_by: Id::from_string(row.try_get::<String, _>("started_by").map_err(sqlx_err)?),
        config_hash: row.try_get("config_hash").map_err(sqlx_err)?,
        status: SsoSigninTestStatus::parse(&status)?,
        report: report.map(|report| report.0),
        started_at: row
            .try_get::<TimestampMicros, _>("started_at")
            .map_err(sqlx_err)?,
        finished_at: row
            .try_get::<Option<TimestampMicros>, _>("finished_at")
            .map_err(sqlx_err)?,
    })
}

#[async_trait]
impl SsoSigninTestRepository for PgSsoSigninTestRepository {
    async fn create(&self, test: &SsoSigninTest) -> Result<()> {
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;

        sqlx::query(
            "INSERT INTO sso_signin_tests
                (id, provider_id, started_by, config_hash, status, report,
                 started_at, finished_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(&test.id.0)
        .bind(&test.provider_id.0)
        .bind(&test.started_by.0)
        .bind(&test.config_hash)
        .bind(test.status.as_str())
        .bind(test.report.as_ref().map(Json))
        .bind(test.started_at)
        .bind(test.finished_at)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?;

        sqlx::query(
            "DELETE FROM sso_signin_tests
              WHERE provider_id = $1
                AND id NOT IN (
                    SELECT id
                      FROM sso_signin_tests
                     WHERE provider_id = $1
                  ORDER BY started_at DESC, id DESC
                     LIMIT $2
                )",
        )
        .bind(&test.provider_id.0)
        .bind(KEEP_PER_PROVIDER)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?;

        tx.commit().await.map_err(sqlx_err)?;
        Ok(())
    }

    async fn get(&self, id: &Id) -> Result<SsoSigninTest> {
        let row = sqlx::query(&format!(
            "SELECT {COLS} FROM sso_signin_tests WHERE id = $1"
        ))
        .bind(&id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?
        .ok_or_else(|| Error::not_found("sso sign-in test"))?;

        row_to_test(row)
    }

    async fn finish(
        &self,
        id: &Id,
        status: SsoSigninTestStatus,
        report: &SsoSigninTestReport,
        at: TimestampMicros,
    ) -> Result<()> {
        let done = sqlx::query(
            "UPDATE sso_signin_tests
                SET status = $2, report = $3, finished_at = $4
              WHERE id = $1",
        )
        .bind(&id.0)
        .bind(status.as_str())
        .bind(Json(report))
        .bind(at)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        if done.rows_affected() == 0 {
            return Err(Error::not_found("sso sign-in test"));
        }

        Ok(())
    }

    async fn started_since(&self, provider_id: &Id, since: TimestampMicros) -> Result<i64> {
        sqlx::query_scalar(
            "SELECT COUNT(*)
               FROM sso_signin_tests
              WHERE provider_id = $1
                AND started_at >= $2",
        )
        .bind(&provider_id.0)
        .bind(since)
        .fetch_one(&self.pool)
        .await
        .map_err(sqlx_err)
    }

    async fn finished_of(&self, provider_ids: &[Id]) -> Result<Vec<SsoSigninTestSummary>> {
        if provider_ids.is_empty() {
            return Ok(Vec::new());
        }

        let provider_ids = provider_ids
            .iter()
            .map(|id| id.0.clone())
            .collect::<Vec<_>>();

        let rows = sqlx::query(
            "SELECT id, provider_id, config_hash, status, finished_at
               FROM sso_signin_tests
              WHERE provider_id = ANY($1::TEXT[])
                AND status <> 'pending'
           ORDER BY finished_at DESC, id DESC",
        )
        .bind(&provider_ids)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        rows.into_iter()
            .map(|row| {
                let status: String = row.try_get("status").map_err(sqlx_err)?;

                Ok(SsoSigninTestSummary {
                    id: Id::from_string(row.try_get::<String, _>("id").map_err(sqlx_err)?),
                    provider_id: Id::from_string(
                        row.try_get::<String, _>("provider_id").map_err(sqlx_err)?,
                    ),
                    config_hash: row.try_get("config_hash").map_err(sqlx_err)?,
                    status: SsoSigninTestStatus::parse(&status)?,
                    finished_at: row
                        .try_get::<Option<TimestampMicros>, _>("finished_at")
                        .map_err(sqlx_err)?,
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::iam::{
        SsoOidcConfig, SsoPlatform, SsoProviderConfig, SsoProviderKind, SsoSamlConfig,
    };

    fn provider() -> SsoProvider {
        SsoProvider {
            id: Id::from_string("p-1"),
            org_id: Id::from_string("org-1"),
            name: "Okta".into(),
            kind: SsoProviderKind::Oidc,
            platform: SsoPlatform::Okta,
            enabled: true,
            config: SsoProviderConfig::Oidc(SsoOidcConfig {
                issuer: "https://idp.example".into(),
                client_id: "molesignal".into(),
                client_secret: "first".into(),
                ..SsoOidcConfig::default()
            }),
            created_at: TimestampMicros(1),
            updated_at: TimestampMicros(1),
        }
    }

    fn oidc(provider: &mut SsoProvider) -> &mut SsoOidcConfig {
        match &mut provider.config {
            SsoProviderConfig::Oidc(config) => config,
            _ => panic!("expected OIDC config"),
        }
    }

    #[test]
    fn the_hash_is_stable_for_the_same_settings() {
        assert_eq!(config_hash(&provider()), config_hash(&provider()));
        assert_eq!(config_hash(&provider()).len(), 64);
    }

    #[test]
    fn a_name_or_the_enabled_switch_leaves_the_hash_alone() {
        let mut changed = provider();
        changed.name = "Okta production".into();
        changed.enabled = false;
        changed.updated_at = TimestampMicros(2);
        assert_eq!(config_hash(&changed), config_hash(&provider()));
    }

    #[test]
    fn every_setting_that_decides_a_sign_in_changes_the_hash() {
        let base = config_hash(&provider());

        let mut secret = provider();
        oidc(&mut secret).client_secret = "second".into();
        assert_ne!(config_hash(&secret), base, "a rotated secret");

        let mut issuer = provider();
        oidc(&mut issuer).issuer = "https://other.example".into();
        assert_ne!(config_hash(&issuer), base, "an endpoint");

        let mut mapping = provider();
        oidc(&mut mapping).field_mapping.email = "mail".into();
        assert_ne!(config_hash(&mapping), base, "the field mapping");

        let mut roles = provider();
        oidc(&mut roles).default_role_id = Some("role-viewer".into());
        assert_ne!(config_hash(&roles), base, "the role binding");

        let mut groups = provider();
        oidc(&mut groups).allowed_groups = vec!["sre".into()];
        assert_ne!(config_hash(&groups), base, "the allowed groups");

        let mut platform = provider();
        platform.platform = SsoPlatform::Custom;
        assert_ne!(config_hash(&platform), base, "the platform's reader");

        let mut kind = provider();
        kind.kind = SsoProviderKind::Saml;
        kind.config = SsoProviderConfig::Saml(SsoSamlConfig::default());
        assert_ne!(config_hash(&kind), base, "the protocol");
    }

    #[test]
    fn a_report_written_by_an_older_version_still_reads() {
        let report: SsoSigninTestReport = serde_json::from_value(serde_json::json!({
            "failure": { "code": "token_exchange" }
        }))
        .expect("report");

        assert_eq!(report.failure.expect("failure").code, "token_exchange");
        assert!(report.identity.is_none() && report.notes.is_empty());
    }
}
