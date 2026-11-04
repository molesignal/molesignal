// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What people keep about services: definitions, error states, error rules
//! and service level objectives. Unlike the projections beside it, nothing here is
//! rebuilt from spans.

use async_trait::async_trait;
use sqlx::{
    Row,
    postgres::{PgRow, types::PgInterval},
    types::Json,
};

use super::{
    PgApmRepository,
    codec::{interval, interval_micros},
    deployments, error_rules, slo,
};
use crate::{
    domain::apm::{
        ApmCatalogRepository, ApmOrgSettings, DeploymentEvent, DeploymentQuery, ErrorIssue,
        ErrorIssueStatus, ErrorRule, KeyTransactionMark, ServiceDefinition, ServiceLevelObjective,
        ServiceLifecycle, ServiceTier,
    },
    infra::persistence::sqlx_err,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

const DEFINITION_COLUMNS: &str = "service_namespace, service_name, description, team_id, \
     owner_id, tier, lifecycle, repository_url, tags, apdex_threshold, key_transactions, \
     archived, updated_by, updated_at";

#[async_trait]
impl ApmCatalogRepository for PgApmRepository {
    async fn list_definitions(&self, org_id: &Id) -> Result<Vec<ServiceDefinition>> {
        let rows = sqlx::query(&format!(
            "SELECT {DEFINITION_COLUMNS} FROM apm_service_definitions
             WHERE org_id = $1
             ORDER BY service_namespace, service_name"
        ))
        .bind(org_id.as_str())
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        rows.iter().map(definition_from_row).collect()
    }

    async fn save_definition(&self, org_id: &Id, definition: &ServiceDefinition) -> Result<()> {
        // Key transactions are written only with a new definition: they change one at a time.
        sqlx::query(
            "INSERT INTO apm_service_definitions (
                 org_id, service_namespace, service_name, description, team_id, owner_id,
                 tier, lifecycle, repository_url, tags, apdex_threshold,
                 key_transactions, archived, updated_by, updated_at
             )
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)
             ON CONFLICT (org_id, service_namespace, service_name) DO UPDATE
             SET description = EXCLUDED.description,
                 team_id = EXCLUDED.team_id,
                 owner_id = EXCLUDED.owner_id,
                 tier = EXCLUDED.tier,
                 lifecycle = EXCLUDED.lifecycle,
                 repository_url = EXCLUDED.repository_url,
                 tags = EXCLUDED.tags,
                 apdex_threshold = EXCLUDED.apdex_threshold,
                 archived = EXCLUDED.archived,
                 updated_by = EXCLUDED.updated_by,
                 updated_at = EXCLUDED.updated_at",
        )
        .bind(org_id.as_str())
        .bind(&definition.namespace)
        .bind(&definition.name)
        .bind(&definition.description)
        .bind(definition.team_id.as_ref().map(Id::as_str))
        .bind(definition.owner_id.as_ref().map(Id::as_str))
        .bind(definition.tier.map(ServiceTier::as_str))
        .bind(definition.lifecycle.map(ServiceLifecycle::as_str))
        .bind(definition.repository_url.as_deref())
        .bind(Json(&definition.tags))
        .bind(definition.apdex_threshold.map(interval))
        .bind(Json(&definition.key_transactions))
        .bind(definition.archived)
        .bind(definition.updated_by.as_str())
        .bind(definition.updated_at)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(())
    }

    async fn mark_key_transaction(
        &self,
        org_id: &Id,
        mark: &KeyTransactionMark,
    ) -> Result<ServiceDefinition> {
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;

        // The row first, so the read below locks it even for a service nobody described.
        sqlx::query(
            "INSERT INTO apm_service_definitions (
                 org_id, service_namespace, service_name, updated_by, updated_at
             )
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (org_id, service_namespace, service_name) DO NOTHING",
        )
        .bind(org_id.as_str())
        .bind(&mark.namespace)
        .bind(&mark.name)
        .bind(mark.updated_by.as_str())
        .bind(mark.updated_at)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?;

        let row = sqlx::query(&format!(
            "SELECT {DEFINITION_COLUMNS} FROM apm_service_definitions
             WHERE org_id = $1 AND service_namespace = $2 AND service_name = $3
             FOR UPDATE"
        ))
        .bind(org_id.as_str())
        .bind(&mark.namespace)
        .bind(&mark.name)
        .fetch_one(&mut *tx)
        .await
        .map_err(sqlx_err)?;

        let mut definition = definition_from_row(&row)?;

        // Already as asked: nothing is written, not even the row added above.
        if !definition.mark_key_transaction(&mark.transaction, mark.key) {
            return Ok(definition);
        }

        definition.updated_by = mark.updated_by.clone();
        definition.updated_at = mark.updated_at;
        definition.validate()?;

        sqlx::query(
            "UPDATE apm_service_definitions
             SET key_transactions = $4, updated_by = $5, updated_at = $6
             WHERE org_id = $1 AND service_namespace = $2 AND service_name = $3",
        )
        .bind(org_id.as_str())
        .bind(&mark.namespace)
        .bind(&mark.name)
        .bind(Json(&definition.key_transactions))
        .bind(definition.updated_by.as_str())
        .bind(definition.updated_at)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?;

        tx.commit().await.map_err(sqlx_err)?;
        Ok(definition)
    }

    async fn delete_definition(&self, org_id: &Id, namespace: &str, name: &str) -> Result<bool> {
        let result = sqlx::query(
            "DELETE FROM apm_service_definitions
             WHERE org_id = $1 AND service_namespace = $2 AND service_name = $3",
        )
        .bind(org_id.as_str())
        .bind(namespace)
        .bind(name)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(result.rows_affected() > 0)
    }

    async fn list_issues(&self, org_id: &Id, fingerprints: &[String]) -> Result<Vec<ErrorIssue>> {
        if fingerprints.is_empty() {
            return Ok(Vec::new());
        }

        let rows = sqlx::query(
            "SELECT fingerprint, status, assignee_id, team_id, resolved_at, updated_by,
                    updated_at
             FROM apm_error_issues
             WHERE org_id = $1 AND fingerprint = ANY($2)",
        )
        .bind(org_id.as_str())
        .bind(fingerprints)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        rows.iter().map(issue_from_row).collect()
    }

    async fn save_issue(&self, org_id: &Id, issue: &ErrorIssue) -> Result<bool> {
        // Only for a group the projection knows: the state hangs off it.
        let result = sqlx::query(
            "INSERT INTO apm_error_issues (
                 org_id, fingerprint, status, assignee_id, team_id, resolved_at,
                 updated_by, updated_at
             )
             SELECT $1, $2, $3, $4, $8, $5, $6, $7
             WHERE EXISTS (
                 SELECT 1 FROM apm_error_groups WHERE org_id = $1 AND fingerprint = $2
             )
             ON CONFLICT (org_id, fingerprint) DO UPDATE
             SET status = EXCLUDED.status,
                 assignee_id = EXCLUDED.assignee_id,
                 team_id = EXCLUDED.team_id,
                 resolved_at = EXCLUDED.resolved_at,
                 updated_by = EXCLUDED.updated_by,
                 updated_at = EXCLUDED.updated_at",
        )
        .bind(org_id.as_str())
        .bind(&issue.fingerprint)
        .bind(issue.status.as_str())
        .bind(issue.assignee_id.as_ref().map(Id::as_str))
        .bind(issue.resolved_at)
        .bind(issue.updated_by.as_str())
        .bind(issue.updated_at)
        .bind(issue.team_id.as_ref().map(Id::as_str))
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(result.rows_affected() > 0)
    }

    async fn list_slos(&self, org_id: &Id) -> Result<Vec<ServiceLevelObjective>> {
        slo::list(&self.pool, org_id).await
    }

    async fn get_slo(&self, org_id: &Id, id: &Id) -> Result<Option<ServiceLevelObjective>> {
        slo::get(&self.pool, org_id, id).await
    }

    async fn save_slo(&self, org_id: &Id, objective: &ServiceLevelObjective) -> Result<()> {
        slo::save(&self.pool, org_id, objective).await
    }

    async fn delete_slo(&self, org_id: &Id, id: &Id) -> Result<bool> {
        slo::delete(&self.pool, org_id, id).await
    }

    async fn list_error_rules(&self, org_id: &Id) -> Result<Vec<ErrorRule>> {
        error_rules::list(&self.pool, Some(org_id))
            .await
            .map(|rules| rules.into_iter().map(|(_, rule)| rule).collect())
    }

    async fn list_all_error_rules(&self) -> Result<Vec<(Id, ErrorRule)>> {
        error_rules::list(&self.pool, None).await
    }

    async fn save_error_rule(&self, org_id: &Id, rule: &ErrorRule) -> Result<()> {
        error_rules::save(&self.pool, org_id, rule).await
    }

    async fn delete_error_rule(&self, org_id: &Id, id: &Id) -> Result<bool> {
        error_rules::delete(&self.pool, org_id, id).await
    }

    async fn order_error_rules(&self, org_id: &Id, ids: &[Id]) -> Result<()> {
        error_rules::order(&self.pool, org_id, ids).await
    }

    async fn save_deployment(&self, org_id: &Id, event: &DeploymentEvent) -> Result<()> {
        deployments::save_deployment(&self.pool, org_id, event).await
    }

    async fn list_deployments(
        &self,
        org_id: &Id,
        query: &DeploymentQuery,
    ) -> Result<Vec<DeploymentEvent>> {
        deployments::list_deployments(&self.pool, org_id, query).await
    }

    async fn get_settings(&self, org_id: &Id) -> Result<Option<ApmOrgSettings>> {
        deployments::get_settings(&self.pool, org_id).await
    }

    async fn save_settings(
        &self,
        org_id: &Id,
        settings: &ApmOrgSettings,
        updated_by: &Id,
    ) -> Result<()> {
        deployments::save_settings(&self.pool, org_id, settings, updated_by).await
    }

    async fn list_all_settings(&self) -> Result<Vec<(Id, ApmOrgSettings)>> {
        deployments::list_all_settings(&self.pool).await
    }
}

fn definition_from_row(row: &PgRow) -> Result<ServiceDefinition> {
    let tier = row.try_get::<Option<String>, _>("tier").map_err(sqlx_err)?;
    let lifecycle = row
        .try_get::<Option<String>, _>("lifecycle")
        .map_err(sqlx_err)?;

    Ok(ServiceDefinition {
        namespace: row.try_get("service_namespace").map_err(sqlx_err)?,
        name: row.try_get("service_name").map_err(sqlx_err)?,
        description: row.try_get("description").map_err(sqlx_err)?,
        team_id: optional_id(row, "team_id")?,
        owner_id: optional_id(row, "owner_id")?,
        tier: tier.as_deref().and_then(ServiceTier::parse),
        lifecycle: lifecycle.as_deref().and_then(ServiceLifecycle::parse),
        repository_url: row.try_get("repository_url").map_err(sqlx_err)?,
        tags: row.try_get::<Json<_>, _>("tags").map_err(sqlx_err)?.0,
        apdex_threshold: row
            .try_get::<Option<PgInterval>, _>("apdex_threshold")
            .map_err(sqlx_err)?
            .and_then(interval_micros),
        key_transactions: row
            .try_get::<Json<_>, _>("key_transactions")
            .map_err(sqlx_err)?
            .0,
        archived: row.try_get("archived").map_err(sqlx_err)?,
        updated_by: Id::from_string(row.try_get::<String, _>("updated_by").map_err(sqlx_err)?),
        updated_at: row
            .try_get::<TimestampMicros, _>("updated_at")
            .map_err(sqlx_err)?,
    })
}

/// A threshold as the column holds it; the domain bounds it far below `i64::MAX`.
fn issue_from_row(row: &PgRow) -> Result<ErrorIssue> {
    let status = row.try_get::<String, _>("status").map_err(sqlx_err)?;

    Ok(ErrorIssue {
        fingerprint: row.try_get("fingerprint").map_err(sqlx_err)?,
        status: ErrorIssueStatus::parse(&status)
            .ok_or_else(|| Error::internal(format!("unknown APM error state {status}")))?,
        assignee_id: optional_id(row, "assignee_id")?,
        team_id: optional_id(row, "team_id")?,
        resolved_at: row
            .try_get::<Option<TimestampMicros>, _>("resolved_at")
            .map_err(sqlx_err)?,
        updated_by: Id::from_string(row.try_get::<String, _>("updated_by").map_err(sqlx_err)?),
        updated_at: row
            .try_get::<TimestampMicros, _>("updated_at")
            .map_err(sqlx_err)?,
    })
}

fn optional_id(row: &PgRow, column: &str) -> Result<Option<Id>> {
    Ok(row
        .try_get::<Option<String>, _>(column)
        .map_err(sqlx_err)?
        .map(Id::from_string))
}
