// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use sqlx::{Row, types::Json};

use super::PgApmRepository;
use crate::{
    domain::apm::{
        CatalogQuery, ErrorGroupQuery, ErrorGroupRecord, ErrorIdentity, ErrorSample,
        ServiceIdentity, ServiceObservation, VersionObservation,
    },
    infra::persistence::sqlx_err,
    shared::{Result, ids::Id, time::TimestampMicros},
};

impl PgApmRepository {
    pub(super) async fn query_services(
        &self,
        query: &CatalogQuery,
    ) -> Result<Vec<ServiceObservation>> {
        let rows = sqlx::query(
            "SELECT service_namespace, service_name, environment,
                    first_seen_at_micros, last_seen_at_micros, runtime_language,
                    telemetry_sdk_name, telemetry_sdk_version, recent_instance_count
             FROM apm_services
             WHERE org_id = $1
               AND last_seen_at_micros >= $2
               AND first_seen_at_micros <= $3
               AND ($4::TEXT IS NULL OR service_namespace = $4)
               AND ($5::TEXT IS NULL OR service_name = $5)
               AND ($6::TEXT IS NULL OR environment = $6)
             ORDER BY service_namespace, service_name, environment",
        )
        .bind(query.org_id.as_str())
        .bind(query.range.start.0)
        .bind(query.range.end.0)
        .bind(query.namespace.as_deref())
        .bind(query.service_name.as_deref())
        .bind(query.environment.as_deref())
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;
        rows.into_iter()
            .map(|row| {
                let count: i32 = row.try_get("recent_instance_count").map_err(sqlx_err)?;
                Ok(ServiceObservation {
                    org_id: query.org_id.clone(),
                    service: ServiceIdentity {
                        namespace: row.try_get("service_namespace").map_err(sqlx_err)?,
                        name: row.try_get("service_name").map_err(sqlx_err)?,
                        environment: row.try_get("environment").map_err(sqlx_err)?,
                    },
                    first_seen_at: TimestampMicros(
                        row.try_get("first_seen_at_micros").map_err(sqlx_err)?,
                    ),
                    last_seen_at: TimestampMicros(
                        row.try_get("last_seen_at_micros").map_err(sqlx_err)?,
                    ),
                    runtime_language: row.try_get("runtime_language").map_err(sqlx_err)?,
                    telemetry_sdk_name: row.try_get("telemetry_sdk_name").map_err(sqlx_err)?,
                    telemetry_sdk_version: row
                        .try_get("telemetry_sdk_version")
                        .map_err(sqlx_err)?,
                    recent_instance_count: u32::try_from(count)
                        .map_err(|_| crate::shared::Error::internal("negative instance count"))?,
                })
            })
            .collect()
    }

    pub(super) async fn query_versions(
        &self,
        query: &CatalogQuery,
    ) -> Result<Vec<VersionObservation>> {
        let rows = sqlx::query(
            "SELECT service_namespace, service_name, environment, version,
                    first_seen_at_micros, last_seen_at_micros, observation_count
             FROM apm_service_versions
             WHERE org_id = $1
               AND last_seen_at_micros >= $2
               AND first_seen_at_micros <= $3
               AND ($4::TEXT IS NULL OR service_namespace = $4)
               AND ($5::TEXT IS NULL OR service_name = $5)
               AND ($6::TEXT IS NULL OR environment = $6)
             ORDER BY last_seen_at_micros DESC, version",
        )
        .bind(query.org_id.as_str())
        .bind(query.range.start.0)
        .bind(query.range.end.0)
        .bind(query.namespace.as_deref())
        .bind(query.service_name.as_deref())
        .bind(query.environment.as_deref())
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;
        rows.into_iter()
            .map(|row| {
                let count: i64 = row.try_get("observation_count").map_err(sqlx_err)?;
                Ok(VersionObservation {
                    org_id: query.org_id.clone(),
                    service: ServiceIdentity {
                        namespace: row.try_get("service_namespace").map_err(sqlx_err)?,
                        name: row.try_get("service_name").map_err(sqlx_err)?,
                        environment: row.try_get("environment").map_err(sqlx_err)?,
                    },
                    version: row.try_get("version").map_err(sqlx_err)?,
                    first_seen_at: TimestampMicros(
                        row.try_get("first_seen_at_micros").map_err(sqlx_err)?,
                    ),
                    last_seen_at: TimestampMicros(
                        row.try_get("last_seen_at_micros").map_err(sqlx_err)?,
                    ),
                    observation_count: u64::try_from(count).map_err(|_| {
                        crate::shared::Error::internal("negative version observation count")
                    })?,
                })
            })
            .collect()
    }

    pub(super) async fn query_error_groups(
        &self,
        query: &ErrorGroupQuery,
    ) -> Result<Vec<ErrorGroupRecord>> {
        let rows = sqlx::query(
            "SELECT service_namespace, service_name, environment, error_identity,
                    first_seen_at_micros, last_seen_at_micros, occurrence_count,
                    representative_message, representative_stack
             FROM apm_error_groups
             WHERE org_id = $1
               AND last_seen_at_micros >= $2
               AND first_seen_at_micros <= $3
               AND ($4::TEXT IS NULL OR service_namespace = $4)
               AND ($5::TEXT IS NULL OR service_name = $5)
               AND ($6::TEXT IS NULL OR environment = $6)
               AND ($7::TEXT IS NULL OR fingerprint = $7)
             ORDER BY last_seen_at_micros DESC, fingerprint",
        )
        .bind(query.org_id.as_str())
        .bind(query.range.start.0)
        .bind(query.range.end.0)
        .bind(query.namespace.as_deref())
        .bind(query.service_name.as_deref())
        .bind(query.environment.as_deref())
        .bind(query.fingerprint.as_deref())
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;
        rows.into_iter()
            .map(|row| {
                let Json(error): Json<ErrorIdentity> =
                    row.try_get("error_identity").map_err(sqlx_err)?;
                let Json(stack): Json<Vec<String>> =
                    row.try_get("representative_stack").map_err(sqlx_err)?;
                let count: i64 = row.try_get("occurrence_count").map_err(sqlx_err)?;
                Ok(ErrorGroupRecord {
                    org_id: query.org_id.clone(),
                    error,
                    service: ServiceIdentity {
                        namespace: row.try_get("service_namespace").map_err(sqlx_err)?,
                        name: row.try_get("service_name").map_err(sqlx_err)?,
                        environment: row.try_get("environment").map_err(sqlx_err)?,
                    },
                    first_seen_at: TimestampMicros(
                        row.try_get("first_seen_at_micros").map_err(sqlx_err)?,
                    ),
                    last_seen_at: TimestampMicros(
                        row.try_get("last_seen_at_micros").map_err(sqlx_err)?,
                    ),
                    occurrence_count: u64::try_from(count)
                        .map_err(|_| crate::shared::Error::internal("negative error count"))?,
                    representative_message: row
                        .try_get("representative_message")
                        .map_err(sqlx_err)?,
                    representative_stack: stack,
                })
            })
            .collect()
    }

    pub(super) async fn query_error_samples(
        &self,
        org_id: &Id,
        fingerprint: &str,
    ) -> Result<Vec<ErrorSample>> {
        let rows = sqlx::query(
            "SELECT s.event_time_micros, s.trace_id, s.span_id, s.trace_available,
                    s.representative_message, s.representative_stack,
                    g.service_namespace, g.service_name, g.environment, g.error_identity
             FROM apm_error_samples s
             JOIN apm_error_groups g
               ON g.org_id = s.org_id AND g.fingerprint = s.fingerprint
             WHERE s.org_id = $1 AND s.fingerprint = $2
             ORDER BY s.event_time_micros DESC, s.sample_slot",
        )
        .bind(org_id.as_str())
        .bind(fingerprint)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;
        rows.into_iter()
            .map(|row| {
                let Json(error): Json<ErrorIdentity> =
                    row.try_get("error_identity").map_err(sqlx_err)?;
                let Json(stack): Json<Vec<String>> =
                    row.try_get("representative_stack").map_err(sqlx_err)?;
                Ok(ErrorSample {
                    org_id: org_id.clone(),
                    error,
                    service: ServiceIdentity {
                        namespace: row.try_get("service_namespace").map_err(sqlx_err)?,
                        name: row.try_get("service_name").map_err(sqlx_err)?,
                        environment: row.try_get("environment").map_err(sqlx_err)?,
                    },
                    event_time: TimestampMicros(
                        row.try_get("event_time_micros").map_err(sqlx_err)?,
                    ),
                    trace_id: row.try_get("trace_id").map_err(sqlx_err)?,
                    span_id: row.try_get("span_id").map_err(sqlx_err)?,
                    trace_available: row.try_get("trace_available").map_err(sqlx_err)?,
                    representative_message: row
                        .try_get("representative_message")
                        .map_err(sqlx_err)?,
                    representative_stack: stack,
                })
            })
            .collect()
    }
}
