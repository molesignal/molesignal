// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! compaction 原子替换与 retention tombstone。
//!
//! 替换是全有全无：只要有一个被替换 Segment 已不是 Active（并发 compaction
//! 抢先提交）就整体回滚返回 Conflict，冲突方必须删掉自己刚写出的新对象。
//! 所有旧对象只入延迟 GC 队列，绝不在事务里直接删除。

use sqlx::{PgPool, Row};

use super::{
    sqlx_err,
    writes::{
        bump_dataset_version, insert_segment, lock_dataset_version, tombstone_segment_artifacts,
    },
};
use crate::{
    domain::storage::{
        ArtifactState, GcReason, OrganizationScope, ReplaceSegments, SegmentState,
        TombstoneSegments,
    },
    shared::{Error, Result},
};

mod transform;

pub(super) use transform::publish_dataset_transform;

pub(super) async fn replace_segments(
    pool: &PgPool,
    scope: &OrganizationScope,
    command: ReplaceSegments,
) -> Result<u64> {
    if command.replaced.is_empty() {
        return Err(Error::invalid("replace_segments requires replaced ids"));
    }
    for segment in &command.replacements {
        segment.validate()?;
        if segment.organization_id != scope.organization_id
            || segment.dataset_id != command.dataset_id
        {
            return Err(Error::invalid(format!(
                "replacement segment {} does not match dataset scope",
                segment.id
            )));
        }
        if segment.primary.state != ArtifactState::Ready {
            return Err(Error::invalid(format!(
                "replacement segment {} primary artifact must be ready",
                segment.id
            )));
        }
    }

    let replaced_ids: Vec<String> = command
        .replaced
        .iter()
        .map(|id| id.as_str().to_owned())
        .collect();

    let mut tx = sqlx::begin(pool).await.map_err(sqlx_err)?;
    let current = lock_dataset_version(&mut tx, scope, &command.dataset_id).await?;
    let new_version = current
        .checked_add(1)
        .ok_or_else(|| Error::internal("catalog version overflow"))?;

    let rows = sqlx::query(
        "SELECT id, state FROM data_segments \
         WHERE org_id = $1 AND dataset_id = $2 AND id = ANY($3) FOR UPDATE",
    )
    .bind(scope.organization_id.as_str())
    .bind(command.dataset_id.as_str())
    .bind(&replaced_ids)
    .fetch_all(&mut *tx)
    .await
    .map_err(sqlx_err)?;
    if rows.len() != replaced_ids.len() {
        return Err(Error::conflict(
            "some replaced segments no longer exist in this dataset",
        ));
    }
    for row in &rows {
        let state: String = row.try_get("state").map_err(sqlx_err)?;
        if state.parse::<SegmentState>()? != SegmentState::Active {
            let id: String = row.try_get("id").map_err(sqlx_err)?;
            return Err(Error::conflict(format!(
                "segment {id} was already {state}; concurrent compaction won"
            )));
        }
    }

    sqlx::query(
        "UPDATE data_segments SET state = 'replaced', retired_at_version = $4 \
         WHERE org_id = $1 AND dataset_id = $2 AND id = ANY($3)",
    )
    .bind(scope.organization_id.as_str())
    .bind(command.dataset_id.as_str())
    .bind(&replaced_ids)
    .bind(super::rows::to_i64(new_version, "retired_at_version")?)
    .execute(&mut *tx)
    .await
    .map_err(sqlx_err)?;

    tombstone_segment_artifacts(
        &mut tx,
        scope,
        &replaced_ids,
        GcReason::Replaced,
        command.gc_not_before_micros,
    )
    .await?;

    for segment in &command.replacements {
        insert_segment(&mut tx, scope, segment, new_version).await?;
    }

    bump_dataset_version(&mut tx, scope, &command.dataset_id, new_version).await?;
    tx.commit().await.map_err(sqlx_err)?;
    Ok(new_version)
}

/// retention / 显式删除：只改状态并入队 GC。幂等——已退役的 Segment 跳过；
/// 全部命中空时不递增版本号。返回当前（或递增后的）catalog_version。
pub(super) async fn tombstone_segments(
    pool: &PgPool,
    scope: &OrganizationScope,
    command: TombstoneSegments,
) -> Result<u64> {
    if command.segment_ids.is_empty() {
        return Err(Error::invalid("tombstone_segments requires segment ids"));
    }
    let segment_ids: Vec<String> = command
        .segment_ids
        .iter()
        .map(|id| id.as_str().to_owned())
        .collect();

    let mut tx = sqlx::begin(pool).await.map_err(sqlx_err)?;
    let current = lock_dataset_version(&mut tx, scope, &command.dataset_id).await?;
    let new_version = current
        .checked_add(1)
        .ok_or_else(|| Error::internal("catalog version overflow"))?;

    let live: Vec<String> = sqlx::query(
        "SELECT id FROM data_segments \
         WHERE org_id = $1 AND dataset_id = $2 AND id = ANY($3) AND state = 'active' FOR UPDATE",
    )
    .bind(scope.organization_id.as_str())
    .bind(command.dataset_id.as_str())
    .bind(&segment_ids)
    .fetch_all(&mut *tx)
    .await
    .map_err(sqlx_err)?
    .iter()
    .map(|row| row.try_get::<String, _>("id").map_err(sqlx_err))
    .collect::<std::result::Result<_, _>>()?;

    if live.is_empty() {
        tx.commit().await.map_err(sqlx_err)?;
        return Ok(current);
    }

    sqlx::query(
        "UPDATE data_segments SET state = 'tombstoned', retired_at_version = $4 \
         WHERE org_id = $1 AND dataset_id = $2 AND id = ANY($3)",
    )
    .bind(scope.organization_id.as_str())
    .bind(command.dataset_id.as_str())
    .bind(&live)
    .bind(super::rows::to_i64(new_version, "retired_at_version")?)
    .execute(&mut *tx)
    .await
    .map_err(sqlx_err)?;

    tombstone_segment_artifacts(
        &mut tx,
        scope,
        &live,
        GcReason::Tombstoned,
        command.gc_not_before_micros,
    )
    .await?;

    bump_dataset_version(&mut tx, scope, &command.dataset_id, new_version).await?;
    tx.commit().await.map_err(sqlx_err)?;
    Ok(new_version)
}
