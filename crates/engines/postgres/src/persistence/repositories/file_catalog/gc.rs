// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use sqlx::{PgPool, Row};

use super::{maintenance, sqlx_err};
use crate::{
    domain::storage::{GcQueueEntry, GcReason, ObjectChecksum, ObjectKey, OrganizationScope},
    shared::Result,
};

pub(super) async fn claim_entries(
    pool: &PgPool,
    scope: &OrganizationScope,
    now_micros: i64,
    lease_expired_before_micros: i64,
    limit: u32,
) -> Result<Vec<GcQueueEntry>> {
    let rows = sqlx::query(
        "WITH candidates AS ( \
           SELECT object_key FROM object_gc_queue \
           WHERE org_id = $1 AND ( \
             (state = 'pending' AND not_before_micros <= $2) OR \
             (state = 'processing' AND updated_at_micros <= $3)) \
           ORDER BY not_before_micros, object_key FOR UPDATE SKIP LOCKED LIMIT $4) \
         UPDATE object_gc_queue q SET state = 'processing', updated_at_micros = $2 \
         FROM candidates c WHERE q.org_id = $1 AND q.object_key = c.object_key \
         RETURNING q.object_key, q.checksum, q.reason, q.not_before_micros, \
                   q.attempt_count, q.last_error",
    )
    .bind(scope.organization_id.as_str())
    .bind(now_micros)
    .bind(lease_expired_before_micros)
    .bind(i64::from(limit.max(1)))
    .fetch_all(pool)
    .await
    .map_err(sqlx_err)?;
    rows.into_iter()
        .map(|row| {
            Ok(GcQueueEntry {
                organization_id: scope.organization_id.clone(),
                object_key: ObjectKey::from_string(
                    row.try_get::<String, _>("object_key").map_err(sqlx_err)?,
                ),
                checksum: row
                    .try_get::<Option<String>, _>("checksum")
                    .map_err(sqlx_err)?
                    .map(ObjectChecksum::from_string),
                reason: parse_reason(&row.try_get::<String, _>("reason").map_err(sqlx_err)?)?,
                not_before_micros: row.try_get("not_before_micros").map_err(sqlx_err)?,
                attempt_count: row.try_get::<i32, _>("attempt_count").map_err(sqlx_err)? as u32,
                last_error: row.try_get("last_error").map_err(sqlx_err)?,
            })
        })
        .collect()
}

pub(super) async fn entry_is_safe(
    pool: &PgPool,
    scope: &OrganizationScope,
    entry: &GcQueueEntry,
) -> Result<bool> {
    let queued = sqlx::query(
        "SELECT checksum, state FROM object_gc_queue WHERE org_id = $1 AND object_key = $2",
    )
    .bind(scope.organization_id.as_str())
    .bind(entry.object_key.as_str())
    .fetch_optional(pool)
    .await
    .map_err(sqlx_err)?;
    let Some(queued) = queued else {
        return Ok(false);
    };
    if queued.try_get::<String, _>("state").map_err(sqlx_err)? != "processing" {
        return Ok(false);
    }
    let checksum = queued
        .try_get::<Option<String>, _>("checksum")
        .map_err(sqlx_err)?;
    if checksum.as_deref() != entry.checksum.as_ref().map(ObjectChecksum::as_str) {
        return Ok(false);
    }
    if maintenance::object_is_referenced(pool, scope, &entry.object_key).await? {
        return Ok(false);
    }
    if let Some(checksum) = &entry.checksum
        && !maintenance::checksum_matches_retired_catalog(pool, scope, &entry.object_key, checksum)
            .await?
    {
        return Ok(false);
    }
    Ok(true)
}

pub(super) async fn complete_entry(
    pool: &PgPool,
    scope: &OrganizationScope,
    object_key: &ObjectKey,
) -> Result<()> {
    sqlx::query(
        "UPDATE object_gc_queue SET state = 'deleted', last_error = NULL, \
         updated_at_micros = $3 WHERE org_id = $1 AND object_key = $2 AND state = 'processing'",
    )
    .bind(scope.organization_id.as_str())
    .bind(object_key.as_str())
    .bind(crate::shared::time::TimestampMicros::now().0)
    .execute(pool)
    .await
    .map_err(sqlx_err)?;
    Ok(())
}

pub(super) async fn retry_entry(
    pool: &PgPool,
    scope: &OrganizationScope,
    object_key: &ObjectKey,
    error: &str,
    not_before_micros: i64,
) -> Result<()> {
    sqlx::query(
        "UPDATE object_gc_queue SET state = 'pending', attempt_count = attempt_count + 1, \
         last_error = $3, not_before_micros = $4, updated_at_micros = $5 \
         WHERE org_id = $1 AND object_key = $2 AND state = 'processing'",
    )
    .bind(scope.organization_id.as_str())
    .bind(object_key.as_str())
    .bind(error)
    .bind(not_before_micros)
    .bind(crate::shared::time::TimestampMicros::now().0)
    .execute(pool)
    .await
    .map_err(sqlx_err)?;
    Ok(())
}

fn parse_reason(value: &str) -> Result<GcReason> {
    match value {
        "replaced" => Ok(GcReason::Replaced),
        "tombstoned" => Ok(GcReason::Tombstoned),
        "index_rebuilt" => Ok(GcReason::IndexRebuilt),
        "orphan" => Ok(GcReason::Orphan),
        other => Err(crate::shared::Error::internal(format!(
            "unknown GC reason {other}"
        ))),
    }
}
