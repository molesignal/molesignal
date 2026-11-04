// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Deployments as pipelines report them, and the APM settings of each
//! organization (the `apm` section of `org_settings`): what people and their
//! pipelines keep, never rebuilt from spans.

use sqlx::{PgPool, Row, postgres::PgRow};

use crate::{
    domain::apm::{
        ApmOrgSettings, ChangeKind, DEPLOYMENTS_LISTED_MAX, DeploymentEvent, DeploymentQuery,
        DeploymentStatus,
    },
    infra::persistence::{
        repositories::org_settings::{self, sections::APM},
        sqlx_err,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub(super) async fn save_deployment(
    pool: &PgPool,
    org_id: &Id,
    event: &DeploymentEvent,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO apm_deployment_events (
             id, org_id, service_namespace, service_name, environment, version, status,
             commit_sha, repository_url, pull_request_url, author, ci_url, description,
             deployed_at, created_by, created_at, kind
         )
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17)",
    )
    .bind(event.id.as_str())
    .bind(org_id.as_str())
    .bind(&event.namespace)
    .bind(&event.service)
    .bind(&event.environment)
    .bind(&event.version)
    .bind(event.status.as_str())
    .bind(event.commit.as_deref())
    .bind(event.repository_url.as_deref())
    .bind(event.pull_request_url.as_deref())
    .bind(event.author.as_deref())
    .bind(event.ci_url.as_deref())
    .bind(event.description.as_deref())
    .bind(event.deployed_at)
    .bind(event.created_by.as_str())
    .bind(event.created_at)
    .bind(event.kind.as_str())
    .execute(pool)
    .await
    .map_err(sqlx_err)?;

    Ok(())
}

pub(super) async fn list_deployments(
    pool: &PgPool,
    org_id: &Id,
    query: &DeploymentQuery,
) -> Result<Vec<DeploymentEvent>> {
    let limit = query.limit.clamp(1, DEPLOYMENTS_LISTED_MAX);
    let rows = sqlx::query(
        "SELECT id, kind, service_namespace, service_name, environment, version, status,
                commit_sha, repository_url, pull_request_url, author, ci_url, description,
                deployed_at, created_by, created_at
         FROM apm_deployment_events
         WHERE org_id = $1
           AND ($2::TEXT IS NULL OR service_namespace IN ($2, ''))
           AND ($3::TEXT IS NULL OR service_name = $3)
           AND ($4::TEXT IS NULL OR environment IN ($4, ''))
           AND ($5::TIMESTAMPTZ IS NULL OR deployed_at >= $5)
           AND ($6::TIMESTAMPTZ IS NULL OR deployed_at <= $6)
         ORDER BY deployed_at DESC, id
         LIMIT $7",
    )
    .bind(org_id.as_str())
    .bind(query.namespace.as_deref())
    .bind(query.service.as_deref())
    .bind(query.environment.as_deref())
    .bind(query.from)
    .bind(query.to)
    .bind(i64::try_from(limit).unwrap_or(i64::MAX))
    .fetch_all(pool)
    .await
    .map_err(sqlx_err)?;

    rows.iter().map(deployment_from_row).collect()
}

fn deployment_from_row(row: &PgRow) -> Result<DeploymentEvent> {
    let status: String = row.try_get("status").map_err(sqlx_err)?;
    let kind: String = row.try_get("kind").map_err(sqlx_err)?;
    let text = |column: &str| row.try_get::<String, _>(column).map_err(sqlx_err);
    let optional = |column: &str| row.try_get::<Option<String>, _>(column).map_err(sqlx_err);

    Ok(DeploymentEvent {
        id: Id::from_string(text("id")?),
        kind: ChangeKind::parse(&kind)
            .ok_or_else(|| Error::internal(format!("unknown change kind {kind}")))?,
        namespace: text("service_namespace")?,
        service: text("service_name")?,
        environment: text("environment")?,
        version: text("version")?,
        status: DeploymentStatus::parse(&status)
            .ok_or_else(|| Error::internal(format!("unknown deployment status {status}")))?,
        commit: optional("commit_sha")?,
        repository_url: optional("repository_url")?,
        pull_request_url: optional("pull_request_url")?,
        author: optional("author")?,
        ci_url: optional("ci_url")?,
        description: optional("description")?,
        deployed_at: row
            .try_get::<TimestampMicros, _>("deployed_at")
            .map_err(sqlx_err)?,
        created_by: Id::from_string(text("created_by")?),
        created_at: row
            .try_get::<TimestampMicros, _>("created_at")
            .map_err(sqlx_err)?,
    })
}

pub(super) async fn get_settings(pool: &PgPool, org_id: &Id) -> Result<Option<ApmOrgSettings>> {
    Ok(org_settings::read(pool, org_id.as_str(), APM)
        .await?
        .map(|section| section.value))
}

pub(super) async fn save_settings(
    pool: &PgPool,
    org_id: &Id,
    settings: &ApmOrgSettings,
    updated_by: &Id,
) -> Result<()> {
    org_settings::write(
        pool,
        org_id.as_str(),
        APM,
        settings,
        Some(updated_by.as_str()),
        TimestampMicros::now(),
    )
    .await
}

pub(super) async fn list_all_settings(pool: &PgPool) -> Result<Vec<(Id, ApmOrgSettings)>> {
    Ok(org_settings::read_all(pool, APM)
        .await?
        .into_iter()
        .map(|(org_id, section)| (Id::from_string(org_id), section.value))
        .collect())
}
