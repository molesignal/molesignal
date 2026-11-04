// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 修复动作、Kubernetes 集群与在事件上发起的修复动作。
//!
//! HTTP 动作的秘密（请求头的值、签名密钥）与集群的 Token 用根密钥加密后单独存；列表里读出的动作
//! 只有请求头的名字，`secrets_set` 说哪些已经设了，执行时才带着秘密读出。

use std::collections::BTreeMap;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row, types::Json};

use super::sqlx_err;
use crate::{
    domain::alerting::remediation::{
        KubernetesCluster, RemediationAction, RemediationParameter, RemediationRepository,
        RemediationRisk, RemediationRun, RemediationRunStatus, RemediationTarget, VerifyBy,
    },
    infra::cipher::CipherRootKey,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub struct PgRemediationRepository {
    pool: PgPool,
    cipher: CipherRootKey,
}

impl PgRemediationRepository {
    pub fn new(pool: PgPool, cipher: CipherRootKey) -> Self {
        Self { pool, cipher }
    }

    fn seal(&self, plaintext: &[u8]) -> Result<(Vec<u8>, Vec<u8>)> {
        self.cipher
            .seal(plaintext)
            .map_err(|error| Error::internal(format!("remediation secret seal: {error}")))
    }

    fn open(&self, nonce: &[u8], ciphertext: &[u8]) -> Result<Vec<u8>> {
        self.cipher
            .open(nonce, ciphertext)
            .map_err(|_| Error::internal("remediation secret decrypt failed"))
    }
}

/// 一个 HTTP 动作的秘密。
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HttpSecrets {
    #[serde(default)]
    headers: BTreeMap<String, String>,
    #[serde(default)]
    signing_secret: Option<String>,
}

/// 把秘密从 target 里拿出来：target 里留下请求头的名字（值为空）。
fn split_secrets(target: &RemediationTarget) -> (RemediationTarget, Option<HttpSecrets>) {
    match target {
        RemediationTarget::Http(http) => {
            let mut plain = http.clone();
            let secrets = HttpSecrets {
                headers: std::mem::take(&mut plain.headers),
                signing_secret: plain.signing_secret.take(),
            };

            plain.headers = secrets
                .headers
                .keys()
                .map(|name| (name.clone(), String::new()))
                .collect();

            let empty = secrets.headers.is_empty() && secrets.signing_secret.is_none();
            (RemediationTarget::Http(plain), (!empty).then_some(secrets))
        }
        other => (other.clone(), None),
    }
}

fn secrets_set(target: &RemediationTarget, secrets: Option<&HttpSecrets>) -> Vec<String> {
    let (RemediationTarget::Http(_), Some(secrets)) = (target, secrets) else {
        return Vec::new();
    };

    let mut set = secrets
        .headers
        .keys()
        .map(|name| format!("header:{name}"))
        .collect::<Vec<_>>();

    if secrets
        .signing_secret
        .as_deref()
        .is_some_and(|secret| !secret.is_empty())
    {
        set.push("signingSecret".into());
    }

    set
}

const ACTION_COLS: &str = "id, org_id, name, description, target, secret_nonce, secret_ciphertext,
     parameters, services, team_ids, risk, verify_after_secs, verify_by, enabled, created_by,
     updated_by, created_at, updated_at";

impl PgRemediationRepository {
    /// 读出一行动作；`reveal` 时把秘密放回 target。
    fn row_to_action(
        &self,
        row: &sqlx::postgres::PgRow,
        reveal: bool,
    ) -> Result<RemediationAction> {
        let Json(mut target): Json<RemediationTarget> = row.try_get("target").map_err(sqlx_err)?;
        let nonce: Option<Vec<u8>> = row.try_get("secret_nonce").map_err(sqlx_err)?;
        let ciphertext: Option<Vec<u8>> = row.try_get("secret_ciphertext").map_err(sqlx_err)?;

        let secrets = match (nonce, ciphertext) {
            (Some(nonce), Some(ciphertext)) => Some(
                serde_json::from_slice::<HttpSecrets>(&self.open(&nonce, &ciphertext)?)
                    .map_err(|_| Error::internal("remediation secrets are not valid JSON"))?,
            ),
            _ => None,
        };

        let set = secrets_set(&target, secrets.as_ref());

        if reveal && let (RemediationTarget::Http(http), Some(secrets)) = (&mut target, secrets) {
            http.headers = secrets.headers;
            http.signing_secret = secrets.signing_secret;
        }

        let Json(parameters): Json<Vec<RemediationParameter>> =
            row.try_get("parameters").map_err(sqlx_err)?;
        let Json(services): Json<Vec<String>> = row.try_get("services").map_err(sqlx_err)?;
        let Json(team_ids): Json<Vec<Id>> = row.try_get("team_ids").map_err(sqlx_err)?;
        let risk: String = row.try_get("risk").map_err(sqlx_err)?;
        let verify_by: String = row.try_get("verify_by").map_err(sqlx_err)?;
        let verify_after: i32 = row.try_get("verify_after_secs").map_err(sqlx_err)?;

        Ok(RemediationAction {
            id: Id(row.try_get("id").map_err(sqlx_err)?),
            org_id: Id(row.try_get("org_id").map_err(sqlx_err)?),
            name: row.try_get("name").map_err(sqlx_err)?,
            description: row.try_get("description").map_err(sqlx_err)?,
            target,
            parameters,
            services,
            team_ids,
            risk: if risk == "l3" {
                RemediationRisk::L3
            } else {
                RemediationRisk::L2
            },
            verify_after_secs: u32::try_from(verify_after).unwrap_or_default(),
            verify_by: if verify_by == "rule_healthy" {
                VerifyBy::RuleHealthy
            } else {
                VerifyBy::IncidentResolved
            },
            enabled: row.try_get("enabled").map_err(sqlx_err)?,
            secrets_set: set,
            created_by: row
                .try_get::<Option<String>, _>("created_by")
                .map_err(sqlx_err)?
                .map(Id),
            updated_by: row
                .try_get::<Option<String>, _>("updated_by")
                .map_err(sqlx_err)?
                .map(Id),
            created_at: row
                .try_get::<TimestampMicros, _>("created_at")
                .map_err(sqlx_err)?,
            updated_at: row
                .try_get::<TimestampMicros, _>("updated_at")
                .map_err(sqlx_err)?,
        })
    }
}

const CLUSTER_COLS: &str = "id, org_id, name, api_server, ca_cert, token_nonce, token_ciphertext,
     created_at, updated_at";

const RUN_COLS: &str =
    "id, org_id, incident_id, action_id, action_name, approval_id, inputs, status,
     output, verify_at, verification, requested_by, executed_by, created_at,
     updated_at";

fn row_to_run(row: &sqlx::postgres::PgRow) -> Result<RemediationRun> {
    let Json(inputs): Json<BTreeMap<String, String>> = row.try_get("inputs").map_err(sqlx_err)?;
    let status: String = row.try_get("status").map_err(sqlx_err)?;

    Ok(RemediationRun {
        id: Id(row.try_get("id").map_err(sqlx_err)?),
        org_id: Id(row.try_get("org_id").map_err(sqlx_err)?),
        incident_id: Id(row.try_get("incident_id").map_err(sqlx_err)?),
        action_id: Id(row.try_get("action_id").map_err(sqlx_err)?),
        action_name: row.try_get("action_name").map_err(sqlx_err)?,
        approval_id: Id(row.try_get("approval_id").map_err(sqlx_err)?),
        inputs,
        status: RemediationRunStatus::parse(&status),
        output: row.try_get("output").map_err(sqlx_err)?,
        verify_at: row
            .try_get::<Option<TimestampMicros>, _>("verify_at")
            .map_err(sqlx_err)?,
        verification: row.try_get("verification").map_err(sqlx_err)?,
        requested_by: Id(row.try_get("requested_by").map_err(sqlx_err)?),
        executed_by: row
            .try_get::<Option<String>, _>("executed_by")
            .map_err(sqlx_err)?
            .map(Id),
        created_at: row
            .try_get::<TimestampMicros, _>("created_at")
            .map_err(sqlx_err)?,
        updated_at: row
            .try_get::<TimestampMicros, _>("updated_at")
            .map_err(sqlx_err)?,
    })
}

#[async_trait]
impl RemediationRepository for PgRemediationRepository {
    async fn list_actions(&self, org_id: &Id) -> Result<Vec<RemediationAction>> {
        let rows = sqlx::query(&format!(
            "SELECT {ACTION_COLS} FROM remediation_actions WHERE org_id = $1 ORDER BY name"
        ))
        .bind(&org_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        rows.iter()
            .map(|row| self.row_to_action(row, false))
            .collect()
    }

    async fn get_action(&self, org_id: &Id, id: &Id) -> Result<Option<RemediationAction>> {
        sqlx::query(&format!(
            "SELECT {ACTION_COLS} FROM remediation_actions WHERE org_id = $1 AND id = $2"
        ))
        .bind(&org_id.0)
        .bind(&id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?
        .map(|row| self.row_to_action(&row, true))
        .transpose()
    }

    async fn save_action(&self, action: RemediationAction) -> Result<RemediationAction> {
        let (plain, secrets) = split_secrets(&action.target);

        let sealed = secrets
            .as_ref()
            .map(|secrets| {
                serde_json::to_vec(secrets)
                    .map_err(|error| Error::internal(format!("remediation secrets: {error}")))
                    .and_then(|bytes| self.seal(&bytes))
            })
            .transpose()?;

        sqlx::query(
            "INSERT INTO remediation_actions
                (id, org_id, name, description, target, secret_nonce, secret_ciphertext,
                 parameters, services, team_ids, risk, verify_after_secs, verify_by, enabled,
                 created_by, updated_by, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18)
             ON CONFLICT (id) DO UPDATE
             SET name = EXCLUDED.name, description = EXCLUDED.description,
                 target = EXCLUDED.target, secret_nonce = EXCLUDED.secret_nonce,
                 secret_ciphertext = EXCLUDED.secret_ciphertext,
                 parameters = EXCLUDED.parameters, services = EXCLUDED.services,
                 team_ids = EXCLUDED.team_ids, risk = EXCLUDED.risk,
                 verify_after_secs = EXCLUDED.verify_after_secs, verify_by = EXCLUDED.verify_by,
                 enabled = EXCLUDED.enabled, updated_by = EXCLUDED.updated_by,
                 updated_at = EXCLUDED.updated_at
             WHERE remediation_actions.org_id = EXCLUDED.org_id",
        )
        .bind(&action.id.0)
        .bind(&action.org_id.0)
        .bind(&action.name)
        .bind(&action.description)
        .bind(Json(&plain))
        .bind(sealed.as_ref().map(|(nonce, _)| nonce.clone()))
        .bind(sealed.as_ref().map(|(_, ciphertext)| ciphertext.clone()))
        .bind(Json(&action.parameters))
        .bind(Json(&action.services))
        .bind(Json(&action.team_ids))
        .bind(match action.risk {
            RemediationRisk::L2 => "l2",
            RemediationRisk::L3 => "l3",
        })
        .bind(i32::try_from(action.verify_after_secs).unwrap_or(i32::MAX))
        .bind(match action.verify_by {
            VerifyBy::IncidentResolved => "incident_resolved",
            VerifyBy::RuleHealthy => "rule_healthy",
        })
        .bind(action.enabled)
        .bind(action.created_by.as_ref().map(|id| &id.0))
        .bind(action.updated_by.as_ref().map(|id| &id.0))
        .bind(action.created_at)
        .bind(action.updated_at)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        self.get_action(&action.org_id, &action.id)
            .await?
            .ok_or_else(|| Error::not_found("remediation action not found"))
    }

    async fn delete_action(&self, org_id: &Id, id: &Id) -> Result<bool> {
        let deleted = sqlx::query("DELETE FROM remediation_actions WHERE org_id = $1 AND id = $2")
            .bind(&org_id.0)
            .bind(&id.0)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?
            .rows_affected();

        Ok(deleted > 0)
    }

    async fn list_clusters(&self, org_id: &Id) -> Result<Vec<KubernetesCluster>> {
        let rows = sqlx::query(&format!(
            "SELECT {CLUSTER_COLS} FROM kubernetes_clusters WHERE org_id = $1 ORDER BY name"
        ))
        .bind(&org_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        rows.iter()
            .map(|row| self.row_to_cluster(row, false))
            .collect()
    }

    async fn get_cluster(&self, org_id: &Id, id: &Id) -> Result<Option<KubernetesCluster>> {
        sqlx::query(&format!(
            "SELECT {CLUSTER_COLS} FROM kubernetes_clusters WHERE org_id = $1 AND id = $2"
        ))
        .bind(&org_id.0)
        .bind(&id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?
        .map(|row| self.row_to_cluster(&row, true))
        .transpose()
    }

    async fn save_cluster(&self, cluster: KubernetesCluster) -> Result<KubernetesCluster> {
        let (nonce, ciphertext) = self.seal(cluster.token.as_bytes())?;

        sqlx::query(
            "INSERT INTO kubernetes_clusters
                (id, org_id, name, api_server, ca_cert, token_nonce, token_ciphertext,
                 created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
             ON CONFLICT (id) DO UPDATE
             SET name = EXCLUDED.name, api_server = EXCLUDED.api_server,
                 ca_cert = EXCLUDED.ca_cert, token_nonce = EXCLUDED.token_nonce,
                 token_ciphertext = EXCLUDED.token_ciphertext,
                 updated_at = EXCLUDED.updated_at
             WHERE kubernetes_clusters.org_id = EXCLUDED.org_id",
        )
        .bind(&cluster.id.0)
        .bind(&cluster.org_id.0)
        .bind(&cluster.name)
        .bind(&cluster.api_server)
        .bind(&cluster.ca_cert)
        .bind(nonce)
        .bind(ciphertext)
        .bind(cluster.created_at)
        .bind(cluster.updated_at)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        self.get_cluster(&cluster.org_id, &cluster.id)
            .await?
            .ok_or_else(|| Error::not_found("cluster not found"))
    }

    async fn delete_cluster(&self, org_id: &Id, id: &Id) -> Result<bool> {
        let deleted = sqlx::query("DELETE FROM kubernetes_clusters WHERE org_id = $1 AND id = $2")
            .bind(&org_id.0)
            .bind(&id.0)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?
            .rows_affected();

        Ok(deleted > 0)
    }

    async fn create_run(&self, run: RemediationRun) -> Result<RemediationRun> {
        self.write_run(&run, true).await?;
        Ok(run)
    }

    async fn update_run(&self, run: RemediationRun) -> Result<RemediationRun> {
        self.write_run(&run, false).await?;
        Ok(run)
    }

    async fn find_run_by_approval(
        &self,
        org_id: &Id,
        approval_id: &Id,
    ) -> Result<Option<RemediationRun>> {
        sqlx::query(&format!(
            "SELECT {RUN_COLS} FROM remediation_runs WHERE org_id = $1 AND approval_id = $2"
        ))
        .bind(&org_id.0)
        .bind(&approval_id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?
        .as_ref()
        .map(row_to_run)
        .transpose()
    }

    async fn list_runs_for_incident(
        &self,
        org_id: &Id,
        incident_id: &Id,
    ) -> Result<Vec<RemediationRun>> {
        let rows = sqlx::query(&format!(
            "SELECT {RUN_COLS} FROM remediation_runs
              WHERE org_id = $1 AND incident_id = $2
              ORDER BY created_at DESC
              LIMIT 100"
        ))
        .bind(&org_id.0)
        .bind(&incident_id.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        rows.iter().map(row_to_run).collect()
    }

    async fn due_verifications(
        &self,
        now: TimestampMicros,
        limit: i64,
    ) -> Result<Vec<RemediationRun>> {
        let rows = sqlx::query(&format!(
            "SELECT {RUN_COLS} FROM remediation_runs
              WHERE status = 'verifying' AND verify_at <= $1
              ORDER BY verify_at
              LIMIT $2"
        ))
        .bind(now)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        rows.iter().map(row_to_run).collect()
    }

    async fn awaiting_approval(&self, limit: i64) -> Result<Vec<RemediationRun>> {
        let rows = sqlx::query(&format!(
            "SELECT {RUN_COLS} FROM remediation_runs
              WHERE status = 'awaiting_approval'
              ORDER BY created_at
              LIMIT $1"
        ))
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        rows.iter().map(row_to_run).collect()
    }
}

impl PgRemediationRepository {
    fn row_to_cluster(
        &self,
        row: &sqlx::postgres::PgRow,
        reveal: bool,
    ) -> Result<KubernetesCluster> {
        let token = if reveal {
            let nonce: Vec<u8> = row.try_get("token_nonce").map_err(sqlx_err)?;
            let ciphertext: Vec<u8> = row.try_get("token_ciphertext").map_err(sqlx_err)?;

            String::from_utf8(self.open(&nonce, &ciphertext)?)
                .map_err(|_| Error::internal("cluster token is not text"))?
        } else {
            String::new()
        };

        Ok(KubernetesCluster {
            id: Id(row.try_get("id").map_err(sqlx_err)?),
            org_id: Id(row.try_get("org_id").map_err(sqlx_err)?),
            name: row.try_get("name").map_err(sqlx_err)?,
            api_server: row.try_get("api_server").map_err(sqlx_err)?,
            ca_cert: row.try_get("ca_cert").map_err(sqlx_err)?,
            token,
            created_at: row
                .try_get::<TimestampMicros, _>("created_at")
                .map_err(sqlx_err)?,
            updated_at: row
                .try_get::<TimestampMicros, _>("updated_at")
                .map_err(sqlx_err)?,
        })
    }

    async fn write_run(&self, run: &RemediationRun, insert: bool) -> Result<()> {
        if !insert {
            sqlx::query(
                "UPDATE remediation_runs
                    SET status = $3, output = $4, verify_at = $5, verification = $6,
                        executed_by = $7, updated_at = $8
                  WHERE id = $1 AND org_id = $2",
            )
            .bind(&run.id.0)
            .bind(&run.org_id.0)
            .bind(run.status.as_str())
            .bind(&run.output)
            .bind(run.verify_at)
            .bind(&run.verification)
            .bind(run.executed_by.as_ref().map(|id| &id.0))
            .bind(run.updated_at)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;

            return Ok(());
        }

        sqlx::query(
            "INSERT INTO remediation_runs
                (id, org_id, incident_id, action_id, action_name, approval_id, inputs, status,
                 output, verify_at, verification, requested_by, executed_by,
                 created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)",
        )
        .bind(&run.id.0)
        .bind(&run.org_id.0)
        .bind(&run.incident_id.0)
        .bind(&run.action_id.0)
        .bind(&run.action_name)
        .bind(&run.approval_id.0)
        .bind(Json(&run.inputs))
        .bind(run.status.as_str())
        .bind(&run.output)
        .bind(run.verify_at)
        .bind(&run.verification)
        .bind(&run.requested_by.0)
        .bind(run.executed_by.as_ref().map(|id| &id.0))
        .bind(run.created_at)
        .bind(run.updated_at)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::alerting::remediation::{HttpRemediation, HttpVerb};

    #[test]
    fn header_values_and_the_signing_secret_leave_the_plain_target() {
        let target = RemediationTarget::Http(HttpRemediation {
            method: HttpVerb::Post,
            url: "https://ci.example.com/job".into(),
            headers: BTreeMap::from([("Authorization".into(), "Bearer abc".into())]),
            body: String::new(),
            signing_secret: Some("s3cret".into()),
        });

        let (plain, secrets) = split_secrets(&target);
        let RemediationTarget::Http(plain) = plain else {
            panic!("http stays http");
        };

        assert_eq!(
            plain.headers.get("Authorization").map(String::as_str),
            Some("")
        );
        assert!(plain.signing_secret.is_none());

        let secrets = secrets.expect("secrets kept apart");
        assert_eq!(
            secrets.headers.get("Authorization").map(String::as_str),
            Some("Bearer abc")
        );
        assert_eq!(
            secrets_set(&target, Some(&secrets)),
            vec![
                "header:Authorization".to_string(),
                "signingSecret".to_string()
            ]
        );
    }
}
