// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Artifact 状态更新：索引构建完成 / 失败 / 重试复位 / 重建替换。
//!
//! 所有流转都校验 [`ArtifactState::can_transition_to`]；重建替换在同一事务里
//! tombstone 旧行、入 GC 队列、插入新行，靠 partial unique 槽位约束保证同一
//! (segment, role, artifact_type) 只有一个存活 Artifact。

use sqlx::{PgConnection, PgPool, Row};

use super::{
    rows::to_i64,
    sqlx_err,
    writes::{enqueue_object_gc, insert_artifact},
};
use crate::{
    domain::storage::{
        ArtifactState, ArtifactUpdate, GcReason, ObjectChecksum, ObjectKey, OrganizationScope,
        UpdateArtifact,
    },
    shared::{Error, Result, time::TimestampMicros},
};

struct CurrentArtifact {
    state: ArtifactState,
    role: String,
    artifact_type: String,
    object_key: String,
    checksum: String,
    source_artifact_id: Option<String>,
    source_checksum: Option<String>,
}

/// 锁定并读取目标 Artifact，同时校验它确实挂在该 org / dataset / segment 下。
async fn lock_current(
    conn: &mut PgConnection,
    scope: &OrganizationScope,
    command: &UpdateArtifact,
) -> Result<CurrentArtifact> {
    let row = sqlx::query(
        "SELECT a.state, a.role, a.artifact_type, a.object_key, a.checksum, \
                a.source_artifact_id, a.source_checksum
         FROM artifacts a
         JOIN data_segments s ON s.org_id = a.org_id AND s.id = a.segment_id
         WHERE a.org_id = $1 AND a.id = $2 AND a.segment_id = $3 AND s.dataset_id = $4
           AND s.state = 'active'
         FOR UPDATE OF a",
    )
    .bind(scope.organization_id.as_str())
    .bind(command.artifact_id.as_str())
    .bind(command.segment_id.as_str())
    .bind(command.dataset_id.as_str())
    .fetch_optional(conn)
    .await
    .map_err(sqlx_err)?
    .ok_or_else(|| {
        Error::not_found(format!(
            "artifact {} in segment {} of dataset {}",
            command.artifact_id, command.segment_id, command.dataset_id
        ))
    })?;
    Ok(CurrentArtifact {
        state: row
            .try_get::<String, _>("state")
            .map_err(sqlx_err)?
            .parse()?,
        role: row.try_get("role").map_err(sqlx_err)?,
        artifact_type: row.try_get("artifact_type").map_err(sqlx_err)?,
        object_key: row.try_get("object_key").map_err(sqlx_err)?,
        checksum: row.try_get("checksum").map_err(sqlx_err)?,
        source_artifact_id: row.try_get("source_artifact_id").map_err(sqlx_err)?,
        source_checksum: row.try_get("source_checksum").map_err(sqlx_err)?,
    })
}

fn ensure_transition(current: ArtifactState, next: ArtifactState) -> Result<()> {
    if !current.can_transition_to(next) {
        return Err(Error::conflict(format!(
            "artifact state transition {} -> {} is not allowed",
            current.as_str(),
            next.as_str()
        )));
    }
    Ok(())
}

pub(super) async fn update_artifact(
    pool: &PgPool,
    scope: &OrganizationScope,
    command: UpdateArtifact,
) -> Result<()> {
    let mut tx = sqlx::begin(pool).await.map_err(sqlx_err)?;
    let current = lock_current(&mut tx, scope, &command).await?;
    let now = TimestampMicros::now().0;

    match &command.update {
        ArtifactUpdate::MarkReady { object } => {
            ensure_transition(current.state, ArtifactState::Ready)?;
            sqlx::query(
                "UPDATE artifacts SET state = 'ready', object_key = $3, size_bytes = $4, \
                 checksum = $5, etag = $6, failure_reason = NULL, updated_at_micros = $7 \
                 WHERE org_id = $1 AND id = $2",
            )
            .bind(scope.organization_id.as_str())
            .bind(command.artifact_id.as_str())
            .bind(object.key.as_str())
            .bind(to_i64(object.size_bytes, "size_bytes")?)
            .bind(object.checksum.as_str())
            .bind(object.etag.as_deref())
            .bind(now)
            .execute(&mut *tx)
            .await
            .map_err(sqlx_err)?;
        }
        ArtifactUpdate::MarkFailed { reason } => {
            ensure_transition(current.state, ArtifactState::Failed)?;
            sqlx::query(
                "UPDATE artifacts SET state = 'failed', failure_reason = $3, \
                 updated_at_micros = $4 WHERE org_id = $1 AND id = $2",
            )
            .bind(scope.organization_id.as_str())
            .bind(command.artifact_id.as_str())
            .bind(reason.as_str())
            .bind(now)
            .execute(&mut *tx)
            .await
            .map_err(sqlx_err)?;
        }
        ArtifactUpdate::Retry => {
            ensure_transition(current.state, ArtifactState::Pending)?;
            sqlx::query(
                "UPDATE artifacts SET state = 'pending', failure_reason = NULL, \
                 updated_at_micros = $3 WHERE org_id = $1 AND id = $2",
            )
            .bind(scope.organization_id.as_str())
            .bind(command.artifact_id.as_str())
            .bind(now)
            .execute(&mut *tx)
            .await
            .map_err(sqlx_err)?;
        }
        ArtifactUpdate::Replace {
            replacement,
            gc_not_before_micros,
        } => {
            ensure_transition(current.state, ArtifactState::Tombstoned)?;
            if replacement.role.as_str() != current.role
                || replacement.artifact_type.as_str() != current.artifact_type
                || replacement
                    .source_artifact_id
                    .as_ref()
                    .map(|id| id.as_str())
                    != current.source_artifact_id.as_deref()
                || replacement
                    .source_checksum
                    .as_ref()
                    .map(ObjectChecksum::as_str)
                    != current.source_checksum.as_deref()
            {
                return Err(Error::invalid(format!(
                    "replacement artifact must keep slot {}/{} and source identity, got {}/{}",
                    current.role,
                    current.artifact_type,
                    replacement.role.as_str(),
                    replacement.artifact_type
                )));
            }
            if replacement.state != ArtifactState::Ready {
                return Err(Error::invalid(
                    "replacement artifact must be uploaded and ready",
                ));
            }
            sqlx::query(
                "UPDATE artifacts SET state = 'tombstoned', updated_at_micros = $3 \
                 WHERE org_id = $1 AND id = $2",
            )
            .bind(scope.organization_id.as_str())
            .bind(command.artifact_id.as_str())
            .bind(now)
            .execute(&mut *tx)
            .await
            .map_err(sqlx_err)?;
            enqueue_object_gc(
                &mut tx,
                scope,
                &ObjectKey::from_string(current.object_key.clone()),
                Some(&ObjectChecksum::from_string(current.checksum.clone())),
                GcReason::IndexRebuilt,
                *gc_not_before_micros,
            )
            .await?;
            insert_artifact(&mut tx, scope, &command.segment_id, replacement).await?;
        }
    }

    tx.commit().await.map_err(sqlx_err)?;
    Ok(())
}
