// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The instances services run as: one row each, its times widened and its
//! attributes merged with every look the projector takes at it.

use std::collections::BTreeMap;

use sqlx::{PgPool, Row, types::Json};

use crate::{
    domain::apm::{CatalogQuery, InstanceObservation, ServiceIdentity},
    infra::persistence::sqlx_err,
    shared::{Result, time::TimestampMicros},
};

pub(super) async fn upsert(pool: &PgPool, instances: &[InstanceObservation]) -> Result<()> {
    if instances.is_empty() {
        return Ok(());
    }

    let mut tx = sqlx::begin(pool).await.map_err(sqlx_err)?;

    for instance in instances {
        sqlx::query(
            "INSERT INTO apm_service_instances (
                org_id, service_namespace, service_name, environment, instance_id, version,
                first_seen_at, last_seen_at, attributes
             ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
             ON CONFLICT (org_id, service_namespace, service_name, environment, instance_id)
             DO UPDATE SET
                version = CASE
                    WHEN EXCLUDED.last_seen_at >= apm_service_instances.last_seen_at
                    THEN COALESCE(EXCLUDED.version, apm_service_instances.version)
                    ELSE apm_service_instances.version
                END,
                first_seen_at = LEAST(
                    apm_service_instances.first_seen_at, EXCLUDED.first_seen_at
                ),
                last_seen_at = GREATEST(
                    apm_service_instances.last_seen_at, EXCLUDED.last_seen_at
                ),
                attributes = apm_service_instances.attributes || EXCLUDED.attributes",
        )
        .bind(instance.org_id.as_str())
        .bind(&instance.service.namespace)
        .bind(&instance.service.name)
        .bind(&instance.service.environment)
        .bind(&instance.instance_id)
        .bind(instance.version.as_deref())
        .bind(instance.first_seen_at)
        .bind(instance.last_seen_at)
        .bind(Json(&instance.attributes))
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?;
    }

    tx.commit().await.map_err(sqlx_err)?;
    Ok(())
}

pub(super) async fn list(pool: &PgPool, query: &CatalogQuery) -> Result<Vec<InstanceObservation>> {
    let rows = sqlx::query(
        "SELECT service_namespace, service_name, environment, instance_id, version,
                first_seen_at, last_seen_at, attributes
         FROM apm_service_instances
         WHERE org_id = $1
           AND last_seen_at >= $2
           AND first_seen_at <= $3
           AND ($4::TEXT IS NULL OR service_namespace = $4)
           AND ($5::TEXT IS NULL OR service_name = $5)
           AND ($6::TEXT IS NULL OR environment = $6)
         ORDER BY service_namespace, service_name, environment, last_seen_at DESC",
    )
    .bind(query.org_id.as_str())
    .bind(query.range.start)
    .bind(query.range.end)
    .bind(query.namespace.as_deref())
    .bind(query.service_name.as_deref())
    .bind(query.environment.as_deref())
    .fetch_all(pool)
    .await
    .map_err(sqlx_err)?;

    rows.into_iter()
        .map(|row| {
            let Json(attributes): Json<BTreeMap<String, String>> =
                row.try_get("attributes").map_err(sqlx_err)?;

            Ok(InstanceObservation {
                org_id: query.org_id.clone(),
                service: ServiceIdentity {
                    namespace: row.try_get("service_namespace").map_err(sqlx_err)?,
                    name: row.try_get("service_name").map_err(sqlx_err)?,
                    environment: row.try_get("environment").map_err(sqlx_err)?,
                },
                instance_id: row.try_get("instance_id").map_err(sqlx_err)?,
                version: row.try_get("version").map_err(sqlx_err)?,
                first_seen_at: row
                    .try_get::<TimestampMicros, _>("first_seen_at")
                    .map_err(sqlx_err)?,
                last_seen_at: row
                    .try_get::<TimestampMicros, _>("last_seen_at")
                    .map_err(sqlx_err)?,
                attributes,
            })
        })
        .collect()
}
