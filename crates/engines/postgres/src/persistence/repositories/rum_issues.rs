// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! PostgreSQL adapter for the issues the errors of RUM applications are grouped into, and for the
//! versions of those applications in the order they were first seen.

use std::collections::HashMap;

use async_trait::async_trait;
use sqlx::{PgPool, Row};

use super::sqlx_err;
use crate::{
    domain::rum::{
        RumIssue, RumIssueChange, RumIssueEnvironment, RumIssueMoment, RumIssueRepository,
        RumIssueSighting, RumIssueStatus, RumVersionRepository,
    },
    shared::{Result, ids::Id, time::TimestampMicros},
};

/// Most versions kept for one application.
const MAX_VERSIONS: i64 = 500;

const COLS: &str = "org_id, id, application_id, status, assignee_id, team_id, overflow,
    error_source, error_type, message, location, first_seen_at, last_seen_at,
    first_version, last_version, resolved_at, resolved_version, resolved_by,
    regressed_at, regressed_version, regressed_environment, updated_by, updated_at";

pub struct PgRumIssueRepository {
    pool: PgPool,
}

impl PgRumIssueRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn optional_id(row: &sqlx::postgres::PgRow, column: &str) -> Option<Id> {
    row.try_get::<Option<String>, _>(column)
        .unwrap_or_default()
        .map(Id)
}

fn optional_micros(row: &sqlx::postgres::PgRow, column: &str) -> Option<TimestampMicros> {
    row.try_get::<Option<TimestampMicros>, _>(column)
        .unwrap_or_default()
}

fn optional_text(row: &sqlx::postgres::PgRow, column: &str) -> Option<String> {
    row.try_get::<Option<String>, _>(column).unwrap_or_default()
}

fn text(row: &sqlx::postgres::PgRow, column: &str) -> String {
    row.try_get::<String, _>(column).unwrap_or_default()
}

fn micros(row: &sqlx::postgres::PgRow, column: &str) -> TimestampMicros {
    row.try_get::<TimestampMicros, _>(column)
        .unwrap_or(TimestampMicros(0))
}

fn moment(row: &sqlx::postgres::PgRow) -> RumIssueMoment {
    RumIssueMoment {
        issue_id: text(row, "issue_id"),
        at: micros(row, "at"),
    }
}

fn row_to(row: sqlx::postgres::PgRow) -> RumIssue {
    RumIssue {
        id: text(&row, "id"),
        application_id: text(&row, "application_id"),
        status: RumIssueStatus::parse(&text(&row, "status")).unwrap_or(RumIssueStatus::Open),
        assignee_id: optional_id(&row, "assignee_id"),
        team_id: optional_id(&row, "team_id"),
        overflow: row.try_get::<bool, _>("overflow").unwrap_or_default(),
        source: text(&row, "error_source"),
        error_type: text(&row, "error_type"),
        message: text(&row, "message"),
        location: text(&row, "location"),
        first_seen_at: micros(&row, "first_seen_at"),
        last_seen_at: micros(&row, "last_seen_at"),
        first_version: optional_text(&row, "first_version"),
        last_version: optional_text(&row, "last_version"),
        resolved_at: optional_micros(&row, "resolved_at"),
        resolved_version: optional_text(&row, "resolved_version"),
        resolved_by: optional_id(&row, "resolved_by"),
        regressed_at: optional_micros(&row, "regressed_at"),
        regressed_version: optional_text(&row, "regressed_version"),
        regressed_environment: optional_text(&row, "regressed_environment"),
        updated_by: optional_id(&row, "updated_by"),
        updated_at: micros(&row, "updated_at"),
    }
}

#[async_trait]
impl RumIssueRepository for PgRumIssueRepository {
    async fn get(&self, org_id: &Id, id: &str) -> Result<Option<RumIssue>> {
        let sql = format!("SELECT {COLS} FROM rum_issues WHERE org_id = $1 AND id = $2");

        let row = sqlx::query(&sql)
            .bind(&org_id.0)
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(sqlx_err)?;

        Ok(row.map(row_to))
    }

    async fn get_many(&self, org_id: &Id, ids: &[String]) -> Result<Vec<RumIssue>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }

        let sql = format!("SELECT {COLS} FROM rum_issues WHERE org_id = $1 AND id = ANY($2)");

        let rows = sqlx::query(&sql)
            .bind(&org_id.0)
            .bind(ids)
            .fetch_all(&self.pool)
            .await
            .map_err(sqlx_err)?;

        Ok(rows.into_iter().map(row_to).collect())
    }

    async fn open(&self, org_id: &Id, issues: &[RumIssue]) -> Result<Vec<RumIssue>> {
        if issues.is_empty() {
            return Ok(Vec::new());
        }

        let mut transaction = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;

        for issue in issues {
            sqlx::query(
                "INSERT INTO rum_issues
                    (org_id, id, application_id, status, overflow, error_source, error_type,
                     message, location, first_seen_at, last_seen_at,
                     first_version, last_version, updated_at)
                 VALUES ($1, $2, $3, 'open', $4, $5, $6, $7, $8, $9, $10, $11, $12, $9)
                 ON CONFLICT (org_id, id) DO NOTHING",
            )
            .bind(&org_id.0)
            .bind(&issue.id)
            .bind(&issue.application_id)
            .bind(issue.overflow)
            .bind(&issue.source)
            .bind(&issue.error_type)
            .bind(&issue.message)
            .bind(&issue.location)
            .bind(issue.first_seen_at)
            .bind(issue.last_seen_at)
            .bind(issue.first_version.as_deref())
            .bind(issue.last_version.as_deref())
            .execute(&mut *transaction)
            .await
            .map_err(sqlx_err)?;
        }

        transaction.commit().await.map_err(sqlx_err)?;

        let ids = issues
            .iter()
            .map(|issue| issue.id.clone())
            .collect::<Vec<_>>();

        self.get_many(org_id, &ids).await
    }

    async fn record_sightings(&self, org_id: &Id, sightings: &[RumIssueSighting]) -> Result<()> {
        if sightings.is_empty() {
            return Ok(());
        }

        let ids = sightings
            .iter()
            .map(|sighting| sighting.id.clone())
            .collect::<Vec<_>>();
        let times = sightings
            .iter()
            .map(|sighting| sighting.at)
            .collect::<Vec<_>>();
        let versions = sightings
            .iter()
            .map(|sighting| sighting.version.clone())
            .collect::<Vec<_>>();

        // The newest sighting decides the last version, whichever order they arrive in.
        sqlx::query(
            "UPDATE rum_issues AS issue
                SET last_version = CASE
                        WHEN seen.at >= issue.last_seen_at AND seen.version IS NOT NULL
                        THEN seen.version
                        ELSE issue.last_version
                    END,
                    last_seen_at = GREATEST(issue.last_seen_at, seen.at)
               FROM unnest($2::text[], $3::timestamptz[], $4::text[]) AS seen(id, at, version)
              WHERE issue.org_id = $1 AND issue.id = seen.id",
        )
        .bind(&org_id.0)
        .bind(&ids)
        .bind(&times)
        .bind(&versions)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(())
    }

    async fn record_environments(
        &self,
        org_id: &Id,
        environments: &[RumIssueEnvironment],
    ) -> Result<()> {
        if environments.is_empty() {
            return Ok(());
        }

        let issues = environments
            .iter()
            .map(|seen| seen.issue_id.clone())
            .collect::<Vec<_>>();
        let applications = environments
            .iter()
            .map(|seen| seen.application_id.clone())
            .collect::<Vec<_>>();
        let names = environments
            .iter()
            .map(|seen| seen.environment.clone())
            .collect::<Vec<_>>();
        let times = environments
            .iter()
            .map(|seen| seen.first_seen_at)
            .collect::<Vec<_>>();

        // Only the first sighting is kept, so a known pair is left as it is.
        sqlx::query(
            "INSERT INTO rum_issue_environments
                (org_id, issue_id, application_id, environment, first_seen_at)
             SELECT $1::varchar, seen.issue_id, seen.application_id, seen.environment, seen.at
               FROM unnest($2::text[], $3::text[], $4::text[], $5::timestamptz[])
                    AS seen(issue_id, application_id, environment, at)
              WHERE EXISTS (SELECT 1 FROM rum_issues issue
                             WHERE issue.org_id = $1 AND issue.id = seen.issue_id)
             ON CONFLICT (org_id, issue_id, environment) DO NOTHING",
        )
        .bind(&org_id.0)
        .bind(&issues)
        .bind(&applications)
        .bind(&names)
        .bind(&times)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(())
    }

    async fn mark_regressed(
        &self,
        org_id: &Id,
        id: &str,
        resolved_at: TimestampMicros,
        at: TimestampMicros,
        version: Option<&str>,
        environment: Option<&str>,
    ) -> Result<Option<RumIssue>> {
        let sql = format!(
            "UPDATE rum_issues
                SET status = 'regressed',
                    regressed_at = $4,
                    regressed_version = $5,
                    regressed_environment = $6,
                    updated_at = $4
              WHERE org_id = $1 AND id = $2
                AND status = 'resolved' AND resolved_at = $3
          RETURNING {COLS}"
        );

        let row = sqlx::query(&sql)
            .bind(&org_id.0)
            .bind(id)
            .bind(resolved_at)
            .bind(at)
            .bind(version)
            .bind(environment)
            .fetch_optional(&self.pool)
            .await
            .map_err(sqlx_err)?;

        Ok(row.map(row_to))
    }

    async fn first_seen_between(
        &self,
        org_id: &Id,
        application_id: &str,
        environment: Option<&str>,
        from: TimestampMicros,
        to: TimestampMicros,
        limit: usize,
    ) -> Result<Vec<RumIssueMoment>> {
        // Without an environment an issue counts when it was first seen anywhere; with one, when
        // it was first seen there.
        let rows = match environment {
            None => {
                sqlx::query(
                    "SELECT id AS issue_id, first_seen_at AS at FROM rum_issues
                      WHERE org_id = $1 AND application_id = $2
                        AND first_seen_at >= $3 AND first_seen_at < $4
                      ORDER BY first_seen_at, id
                      LIMIT $5",
                )
                .bind(&org_id.0)
                .bind(application_id)
                .bind(from)
                .bind(to)
                .bind(i64::try_from(limit).unwrap_or(i64::MAX))
                .fetch_all(&self.pool)
                .await
            }
            Some(environment) => {
                sqlx::query(
                    "SELECT issue_id, first_seen_at AS at FROM rum_issue_environments
                      WHERE org_id = $1 AND application_id = $2 AND environment = $3
                        AND first_seen_at >= $4 AND first_seen_at < $5
                      ORDER BY first_seen_at, issue_id
                      LIMIT $6",
                )
                .bind(&org_id.0)
                .bind(application_id)
                .bind(environment)
                .bind(from)
                .bind(to)
                .bind(i64::try_from(limit).unwrap_or(i64::MAX))
                .fetch_all(&self.pool)
                .await
            }
        }
        .map_err(sqlx_err)?;

        Ok(rows.iter().map(moment).collect())
    }

    async fn regressed_between(
        &self,
        org_id: &Id,
        application_id: &str,
        environment: Option<&str>,
        from: TimestampMicros,
        to: TimestampMicros,
        limit: usize,
    ) -> Result<Vec<RumIssueMoment>> {
        let rows = sqlx::query(
            "SELECT id AS issue_id, regressed_at AS at FROM rum_issues
              WHERE org_id = $1 AND application_id = $2
                AND ($3::text IS NULL OR regressed_environment = $3)
                AND regressed_at >= $4 AND regressed_at < $5
              ORDER BY regressed_at, id
              LIMIT $6",
        )
        .bind(&org_id.0)
        .bind(application_id)
        .bind(environment)
        .bind(from)
        .bind(to)
        .bind(i64::try_from(limit).unwrap_or(i64::MAX))
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(rows.iter().map(moment).collect())
    }

    async fn update(
        &self,
        org_id: &Id,
        id: &str,
        change: &RumIssueChange,
        by: &Id,
        at: TimestampMicros,
    ) -> Result<Option<RumIssue>> {
        let status = change.status.map(RumIssueStatus::as_str);
        let (set_assignee, assignee) = match &change.assignee {
            Some(assignee) => (true, assignee.as_ref().map(|id| id.0.as_str())),
            None => (false, None),
        };
        let (set_team, team) = match &change.team {
            Some(team) => (true, team.as_ref().map(|id| id.0.as_str())),
            None => (false, None),
        };

        // A new state ends what the old one recorded: the resolving of an issue that is open
        // again, the regression of one resolved again. Resolving takes the newest version the
        // application has, so that only a newer one reopens the issue.
        let sql = format!(
            "UPDATE rum_issues AS issue
                SET status = COALESCE($3::text, issue.status),
                    assignee_id = CASE WHEN $4 THEN $5 ELSE issue.assignee_id END,
                    team_id = CASE WHEN $6 THEN $7 ELSE issue.team_id END,
                    resolved_at = CASE
                        WHEN $3::text = 'resolved' THEN $8::timestamptz
                        WHEN $3::text IS NOT NULL THEN NULL
                        ELSE issue.resolved_at END,
                    resolved_version = CASE
                        WHEN $3::text = 'resolved' THEN (
                            SELECT version.version FROM rum_application_versions version
                             WHERE version.org_id = issue.org_id
                               AND version.application_id = issue.application_id
                             ORDER BY version.first_seen_at DESC, version.version DESC
                             LIMIT 1)
                        WHEN $3::text IS NOT NULL THEN NULL
                        ELSE issue.resolved_version END,
                    resolved_by = CASE
                        WHEN $3::text = 'resolved' THEN $9
                        WHEN $3::text IS NOT NULL THEN NULL
                        ELSE issue.resolved_by END,
                    regressed_at = CASE
                        WHEN $3::text IS NOT NULL THEN NULL ELSE issue.regressed_at END,
                    regressed_version = CASE
                        WHEN $3::text IS NOT NULL THEN NULL ELSE issue.regressed_version END,
                    regressed_environment = CASE
                        WHEN $3::text IS NOT NULL THEN NULL ELSE issue.regressed_environment END,
                    updated_by = $9,
                    updated_at = $8::timestamptz
              WHERE issue.org_id = $1 AND issue.id = $2
          RETURNING {COLS}"
        );

        let row = sqlx::query(&sql)
            .bind(&org_id.0)
            .bind(id)
            .bind(status)
            .bind(set_assignee)
            .bind(assignee)
            .bind(set_team)
            .bind(team)
            .bind(at)
            .bind(&by.0)
            .fetch_optional(&self.pool)
            .await
            .map_err(sqlx_err)?;

        Ok(row.map(row_to))
    }
}

pub struct PgRumVersionRepository {
    pool: PgPool,
}

impl PgRumVersionRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl RumVersionRepository for PgRumVersionRepository {
    async fn record(
        &self,
        org_id: &Id,
        application_id: &str,
        versions: &[String],
        at: TimestampMicros,
    ) -> Result<()> {
        if versions.is_empty() {
            return Ok(());
        }

        // An application keeps a bounded number of versions: a client that names a new one with
        // every event does not grow the table.
        sqlx::query(
            "INSERT INTO rum_application_versions
                (org_id, application_id, version, first_seen_at)
             SELECT $1::varchar, $2::varchar, named.version, $4::timestamptz
               FROM unnest($3::text[]) AS named(version)
              LIMIT GREATEST(0, $5 - (
                    SELECT count(*) FROM rum_application_versions
                     WHERE org_id = $1 AND application_id = $2))
             ON CONFLICT DO NOTHING",
        )
        .bind(&org_id.0)
        .bind(application_id)
        .bind(versions)
        .bind(at)
        .bind(MAX_VERSIONS)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(())
    }

    async fn first_seen(
        &self,
        org_id: &Id,
        application_id: &str,
        versions: &[String],
    ) -> Result<HashMap<String, TimestampMicros>> {
        if versions.is_empty() {
            return Ok(HashMap::new());
        }

        let rows = sqlx::query(
            "SELECT version, first_seen_at FROM rum_application_versions
              WHERE org_id = $1 AND application_id = $2 AND version = ANY($3)",
        )
        .bind(&org_id.0)
        .bind(application_id)
        .bind(versions)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(rows
            .into_iter()
            .map(|row| (text(&row, "version"), micros(&row, "first_seen_at")))
            .collect())
    }
}
