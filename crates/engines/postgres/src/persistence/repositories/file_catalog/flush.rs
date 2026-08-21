// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! flush 原子发布。
//!
//! 一笔事务内：写 flush 溯源 → 插 Segment / Artifact → 推进 WAL checkpoint →
//! 递增 catalog_version。`flush_id` 幂等：重试命中已有溯源行时不写任何数据，
//! 直接返回已提交结果。WAL 清理不是提交条件。

use std::collections::HashSet;

use sqlx::{PgPool, Row};

use super::{
    rows::{to_i64, to_u64},
    sqlx_err,
    writes::{bump_dataset_version, insert_segment, lock_dataset_version},
};
use crate::{
    domain::storage::{
        ArtifactState, CommitFlush, FlushCommitResult, OrganizationScope, SegmentState,
    },
    shared::{Error, Result, time::TimestampMicros},
};

pub(super) async fn commit_flush(
    pool: &PgPool,
    scope: &OrganizationScope,
    command: CommitFlush,
) -> Result<FlushCommitResult> {
    validate(scope, &command)?;
    let provenance = &command.provenance;

    let mut tx = sqlx::begin(pool).await.map_err(sqlx_err)?;

    // Serialize every publication for one dataset before inspecting writer epochs. Without this
    // lock, a lower and a higher epoch can both observe the old checkpoint, then the lower epoch
    // can commit after the higher one and bypass the zombie-writer fence.
    let current = lock_dataset_version(&mut tx, scope, &command.dataset_id).await?;

    // 僵尸 writer 栅栏：同一 (dataset, node) 一旦有更高 epoch 提交过 checkpoint，
    // 旧 epoch 的迟到 flush 必须拒绝，防止重启 / 租约切换后旧 writer 复活。
    let fenced_epoch: Option<i64> = sqlx::query(
        "SELECT MAX(writer_epoch) AS max_epoch FROM wal_checkpoints \
         WHERE org_id = $1 AND dataset_id = $2 AND writer_node_id = $3",
    )
    .bind(scope.organization_id.as_str())
    .bind(command.dataset_id.as_str())
    .bind(provenance.writer_node_id.as_str())
    .fetch_one(&mut *tx)
    .await
    .map_err(sqlx_err)?
    .try_get("max_epoch")
    .map_err(sqlx_err)?;
    if let Some(max_epoch) = fenced_epoch
        && to_u64(max_epoch, "writer_epoch")? > provenance.writer_epoch.0
    {
        return Err(Error::conflict(format!(
            "stale writer epoch {} for dataset {} node {}: fenced by epoch {max_epoch}",
            provenance.writer_epoch, command.dataset_id, provenance.writer_node_id
        )));
    }

    let inserted = sqlx::query(
        "INSERT INTO storage_flush_commits (org_id, dataset_id, flush_id, writer_node_id, \
         writer_epoch, sequence_start, sequence_end, committed_at_micros)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
         ON CONFLICT (org_id, dataset_id, flush_id) DO NOTHING",
    )
    .bind(scope.organization_id.as_str())
    .bind(command.dataset_id.as_str())
    .bind(provenance.flush_id.as_str())
    .bind(provenance.writer_node_id.as_str())
    .bind(to_i64(provenance.writer_epoch.0, "writer_epoch")?)
    .bind(to_i64(provenance.sequence.start.0, "sequence_start")?)
    .bind(to_i64(provenance.sequence.end.0, "sequence_end")?)
    .bind(TimestampMicros::now().0)
    .execute(&mut *tx)
    .await
    .map_err(sqlx_err)?
    .rows_affected();

    if inserted == 0 {
        // 幂等重试：flush 已提交过，什么都不写。
        tx.commit().await.map_err(sqlx_err)?;
        return Ok(FlushCommitResult {
            catalog_version: current,
            already_committed: true,
        });
    }

    let new_version = current
        .checked_add(1)
        .ok_or_else(|| Error::internal("catalog version overflow"))?;
    for segment in &command.segments {
        insert_segment(&mut tx, scope, segment, new_version).await?;
    }

    sqlx::query(
        "INSERT INTO wal_checkpoints (org_id, dataset_id, writer_node_id, writer_epoch, \
         committed_sequence, updated_at_micros)
         VALUES ($1, $2, $3, $4, $5, $6)
         ON CONFLICT (org_id, dataset_id, writer_node_id, writer_epoch) DO UPDATE SET
             committed_sequence = GREATEST(wal_checkpoints.committed_sequence, \
                                           EXCLUDED.committed_sequence),
             updated_at_micros = EXCLUDED.updated_at_micros",
    )
    .bind(scope.organization_id.as_str())
    .bind(command.dataset_id.as_str())
    .bind(provenance.writer_node_id.as_str())
    .bind(to_i64(provenance.writer_epoch.0, "writer_epoch")?)
    .bind(to_i64(provenance.sequence.end.0, "committed_sequence")?)
    .bind(TimestampMicros::now().0)
    .execute(&mut *tx)
    .await
    .map_err(sqlx_err)?;

    bump_dataset_version(&mut tx, scope, &command.dataset_id, new_version).await?;
    tx.commit().await.map_err(sqlx_err)?;

    Ok(FlushCommitResult {
        catalog_version: new_version,
        already_committed: false,
    })
}

fn validate(scope: &OrganizationScope, command: &CommitFlush) -> Result<()> {
    let provenance = &command.provenance;
    if provenance.sequence.start.0 > provenance.sequence.end.0 {
        return Err(Error::invalid("flush sequence range is inverted"));
    }
    if command.segments.is_empty() {
        return Err(Error::invalid("flush must publish at least one segment"));
    }
    let mut ordinals = HashSet::with_capacity(command.segments.len());
    for segment in &command.segments {
        segment.validate()?;
        if segment.organization_id != scope.organization_id {
            return Err(Error::invalid(format!(
                "segment {} does not belong to organization scope",
                segment.id
            )));
        }
        if segment.dataset_id != command.dataset_id {
            return Err(Error::invalid(format!(
                "segment {} targets dataset {} but flush targets {}",
                segment.id, segment.dataset_id, command.dataset_id
            )));
        }
        if segment.flush_id.as_ref() != Some(&provenance.flush_id) {
            return Err(Error::invalid(format!(
                "segment {} flush_id does not match provenance {}",
                segment.id, provenance.flush_id
            )));
        }
        if segment.sequence_range != Some(provenance.sequence) {
            return Err(Error::invalid(format!(
                "segment {} sequence range does not match flush provenance",
                segment.id
            )));
        }
        if !ordinals.insert(segment.output_ordinal) {
            return Err(Error::invalid(format!(
                "flush {} contains duplicate output ordinal {}",
                provenance.flush_id, segment.output_ordinal
            )));
        }
        if segment.state != SegmentState::Active {
            return Err(Error::invalid(format!(
                "segment {} must be published as active",
                segment.id
            )));
        }
        if segment.primary.state != ArtifactState::Ready {
            return Err(Error::invalid(format!(
                "segment {} primary artifact must be ready (uploaded) before publish",
                segment.id
            )));
        }
    }
    Ok(())
}
