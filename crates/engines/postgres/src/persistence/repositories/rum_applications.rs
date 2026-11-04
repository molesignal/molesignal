// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! PostgreSQL adapter for the applications that send RUM data.

use async_trait::async_trait;
use sqlx::{PgPool, Row, types::Json};

use super::sqlx_err;
use crate::{
    domain::rum::{
        DEFAULT_ISSUES_PER_HOUR, MAX_SESSION_SAMPLE_RATE, RetentionFilter, RumApplication,
        RumApplicationPatch, RumApplicationRepository, RumApplicationSettings, RumPlatform,
        ScrubKind,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub struct PgRumApplicationRepository {
    pool: PgPool,
}

impl PgRumApplicationRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

const COLS: &str = "org_id, id, name, platform, collect_ip, collect_geo, default_privacy,
                    issues_per_hour, scrub, session_sample_rate, retention_filters, keep_errors,
                    sdk_settings, segments, created_by, created_at, updated_at";

/// The kinds of a stored list, each once in the order they are shown; a name this version does
/// not know is left out.
fn scrub_kinds(names: &[String]) -> Vec<ScrubKind> {
    let kinds = names
        .iter()
        .filter_map(|name| ScrubKind::parse(name))
        .collect::<Vec<_>>();

    ScrubKind::normalized(&kinds)
}

fn scrub_names(kinds: &[ScrubKind]) -> Vec<String> {
    kinds.iter().map(|kind| kind.as_str().to_string()).collect()
}

/// The filters of a stored list that can be read.
fn readable_filters(stored: Vec<serde_json::Value>) -> Vec<RetentionFilter> {
    stored
        .into_iter()
        .filter_map(|filter| serde_json::from_value(filter).ok())
        .collect()
}

fn platform_name(platform: RumPlatform) -> &'static str {
    match platform {
        RumPlatform::Browser => "browser",
        RumPlatform::Android => "android",
        RumPlatform::Ios => "ios",
        RumPlatform::Flutter => "flutter",
        RumPlatform::ReactNative => "react-native",
        RumPlatform::Other => "other",
    }
}

fn parse_platform(name: &str) -> RumPlatform {
    match name {
        "android" => RumPlatform::Android,
        "ios" => RumPlatform::Ios,
        "flutter" => RumPlatform::Flutter,
        "react-native" => RumPlatform::ReactNative,
        "other" => RumPlatform::Other,
        _ => RumPlatform::Browser,
    }
}

fn row_to(row: sqlx::postgres::PgRow) -> RumApplication {
    RumApplication {
        org_id: Id(row.try_get::<String, _>("org_id").unwrap_or_default()),
        id: row.try_get::<String, _>("id").unwrap_or_default(),
        name: row.try_get::<String, _>("name").unwrap_or_default(),
        platform: parse_platform(&row.try_get::<String, _>("platform").unwrap_or_default()),
        settings: RumApplicationSettings {
            collect_ip: row.try_get::<bool, _>("collect_ip").unwrap_or(true),
            collect_geo: row.try_get::<bool, _>("collect_geo").unwrap_or(true),
            default_privacy: row
                .try_get::<String, _>("default_privacy")
                .unwrap_or_else(|_| "mask".into()),
            issues_per_hour: row
                .try_get::<i32, _>("issues_per_hour")
                .ok()
                .and_then(|limit| u32::try_from(limit).ok())
                .unwrap_or(DEFAULT_ISSUES_PER_HOUR),
            scrub: row
                .try_get::<Vec<String>, _>("scrub")
                .map_or_else(|_| ScrubKind::ALL.to_vec(), |names| scrub_kinds(&names)),
            session_sample_rate: row
                .try_get::<i32, _>("session_sample_rate")
                .ok()
                .and_then(|rate| u32::try_from(rate).ok())
                .map_or(MAX_SESSION_SAMPLE_RATE, |rate| {
                    rate.min(MAX_SESSION_SAMPLE_RATE)
                }),
            // A filter this version cannot read is left out, like a kind of scrubbing it does not
            // know: the others still apply.
            retention_filters: row
                .try_get::<Json<Vec<serde_json::Value>>, _>("retention_filters")
                .map(|filters| readable_filters(filters.0))
                .unwrap_or_default(),
            keep_errors: row.try_get::<bool, _>("keep_errors").unwrap_or(true),
            // Settings this version cannot read leave the SDK to what its page gave it.
            sdk: row
                .try_get::<Json<serde_json::Value>, _>("sdk_settings")
                .ok()
                .and_then(|settings| serde_json::from_value(settings.0).ok())
                .unwrap_or_default(),
            // As with the filters: one this version cannot read is left out.
            segments: row
                .try_get::<Json<Vec<serde_json::Value>>, _>("segments")
                .map(|segments| {
                    segments
                        .0
                        .into_iter()
                        .filter_map(|segment| serde_json::from_value(segment).ok())
                        .collect()
                })
                .unwrap_or_default(),
        },
        created_by: row
            .try_get::<Option<String>, _>("created_by")
            .unwrap_or_default()
            .map(Id),
        created_at: row
            .try_get::<TimestampMicros, _>("created_at")
            .unwrap_or_default(),
        updated_at: row
            .try_get::<TimestampMicros, _>("updated_at")
            .unwrap_or_default(),
    }
}

/// A taken id or name is the caller's mistake, not the database's.
fn conflict_or(error: sqlx::Error) -> Error {
    match &error {
        sqlx::Error::Database(database) if database.code().as_deref() == Some("23505") => {
            Error::conflict("an application with this id or name already exists")
        }
        _ => sqlx_err(error),
    }
}

#[async_trait]
impl RumApplicationRepository for PgRumApplicationRepository {
    async fn create(&self, application: RumApplication) -> Result<RumApplication> {
        sqlx::query(
            "INSERT INTO rum_applications
                (org_id, id, name, platform, collect_ip, collect_geo, default_privacy,
                 issues_per_hour, scrub, session_sample_rate, retention_filters, keep_errors,
                 sdk_settings, segments, created_by, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17)",
        )
        .bind(&application.org_id.0)
        .bind(&application.id)
        .bind(&application.name)
        .bind(platform_name(application.platform))
        .bind(application.settings.collect_ip)
        .bind(application.settings.collect_geo)
        .bind(&application.settings.default_privacy)
        .bind(i32::try_from(application.settings.issues_per_hour).unwrap_or(i32::MAX))
        .bind(scrub_names(&application.settings.scrub))
        .bind(i32::try_from(application.settings.session_sample_rate).unwrap_or(100))
        .bind(Json(&application.settings.retention_filters))
        .bind(application.settings.keep_errors)
        .bind(Json(&application.settings.sdk))
        .bind(Json(&application.settings.segments))
        .bind(application.created_by.as_ref().map(|id| id.0.as_str()))
        .bind(application.created_at)
        .bind(application.updated_at)
        .execute(&self.pool)
        .await
        .map_err(conflict_or)?;

        Ok(application)
    }

    async fn get(&self, org_id: &Id, id: &str) -> Result<RumApplication> {
        let sql = format!("SELECT {COLS} FROM rum_applications WHERE org_id = $1 AND id = $2");

        let row = sqlx::query(&sql)
            .bind(&org_id.0)
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(sqlx_err)?;

        row.map(row_to)
            .ok_or_else(|| Error::not_found(format!("RUM application `{id}` does not exist")))
    }

    async fn list(&self, org_id: &Id) -> Result<Vec<RumApplication>> {
        let sql = format!(
            "SELECT {COLS} FROM rum_applications WHERE org_id = $1 ORDER BY created_at, id"
        );

        let rows = sqlx::query(&sql)
            .bind(&org_id.0)
            .fetch_all(&self.pool)
            .await
            .map_err(sqlx_err)?;

        Ok(rows.into_iter().map(row_to).collect())
    }

    async fn update(
        &self,
        org_id: &Id,
        id: &str,
        patch: RumApplicationPatch,
    ) -> Result<RumApplication> {
        let current = self.get(org_id, id).await?;
        let settings = patch.settings.as_ref().map_or_else(
            || current.settings.clone(),
            |change| change.applied_to(&current.settings),
        );

        sqlx::query(
            "UPDATE rum_applications
                SET name = $3, platform = $4, collect_ip = $5, collect_geo = $6,
                    default_privacy = $7, issues_per_hour = $8, scrub = $9,
                    session_sample_rate = $10, retention_filters = $11, keep_errors = $12,
                    sdk_settings = $13, segments = $14, updated_at = $15
              WHERE org_id = $1 AND id = $2",
        )
        .bind(&org_id.0)
        .bind(id)
        .bind(
            patch
                .name
                .as_deref()
                .map(str::trim)
                .unwrap_or(&current.name),
        )
        .bind(platform_name(patch.platform.unwrap_or(current.platform)))
        .bind(settings.collect_ip)
        .bind(settings.collect_geo)
        .bind(&settings.default_privacy)
        .bind(i32::try_from(settings.issues_per_hour).unwrap_or(i32::MAX))
        .bind(scrub_names(&settings.scrub))
        .bind(i32::try_from(settings.session_sample_rate).unwrap_or(100))
        .bind(Json(&settings.retention_filters))
        .bind(settings.keep_errors)
        .bind(Json(&settings.sdk))
        .bind(Json(&settings.segments))
        .bind(TimestampMicros::now())
        .execute(&self.pool)
        .await
        .map_err(conflict_or)?;

        self.get(org_id, id).await
    }

    async fn delete(&self, org_id: &Id, id: &str) -> Result<()> {
        let deleted = sqlx::query("DELETE FROM rum_applications WHERE org_id = $1 AND id = $2")
            .bind(&org_id.0)
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?
            .rows_affected();

        if deleted == 0 {
            return Err(Error::not_found(format!(
                "RUM application `{id}` does not exist"
            )));
        }

        Ok(())
    }
}
