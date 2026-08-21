// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 物理数据集的幂等建集与列表。

use sqlx::{PgPool, types::Json};

use super::{
    rows::{DATASET_COLS, dataset_from_row},
    sqlx_err,
};
use crate::{
    domain::storage::{OrganizationScope, PhysicalDataset, PhysicalDatasetId, PhysicalDatasetSpec},
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

/// `(org, logical_stream, dataset_type)` 幂等建集：已存在的直接返回现有行，
/// 并发建集靠唯一约束收敛到同一行。
pub(super) async fn ensure_datasets(
    pool: &PgPool,
    scope: &OrganizationScope,
    logical_stream_id: &Id,
    specs: &[PhysicalDatasetSpec],
) -> Result<Vec<PhysicalDataset>> {
    let now = TimestampMicros::now().0;
    for spec in specs {
        sqlx::query(
            "INSERT INTO physical_datasets (org_id, id, logical_stream_id, dataset_type, \
             dataset_type_version, partition_policy, storage_policy, index_policy, \
             catalog_version, state, created_at_micros, updated_at_micros)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 0, 'active', $9, $9)
             ON CONFLICT (org_id, logical_stream_id, dataset_type) DO NOTHING",
        )
        .bind(scope.organization_id.as_str())
        .bind(PhysicalDatasetId::generate().as_str())
        .bind(logical_stream_id.as_str())
        .bind(spec.dataset_type.as_str())
        .bind(spec.dataset_type_version as i32)
        .bind(Json(&spec.partition_policy))
        .bind(Json(&spec.storage_policy))
        .bind(Json(&spec.index_policy))
        .bind(now)
        .execute(pool)
        .await
        .map_err(sqlx_err)?;
    }

    let datasets = list_datasets(pool, scope, logical_stream_id).await?;
    specs
        .iter()
        .map(|spec| {
            datasets
                .iter()
                .find(|dataset| dataset.dataset_type == spec.dataset_type)
                .cloned()
                .ok_or_else(|| {
                    Error::internal(format!(
                        "dataset `{}` missing right after ensure for stream {}",
                        spec.dataset_type, logical_stream_id
                    ))
                })
        })
        .collect()
}

pub(super) async fn list_datasets(
    pool: &PgPool,
    scope: &OrganizationScope,
    logical_stream_id: &Id,
) -> Result<Vec<PhysicalDataset>> {
    let rows = sqlx::query(&format!(
        "SELECT {DATASET_COLS} FROM physical_datasets \
         WHERE org_id = $1 AND logical_stream_id = $2 ORDER BY dataset_type"
    ))
    .bind(scope.organization_id.as_str())
    .bind(logical_stream_id.as_str())
    .fetch_all(pool)
    .await
    .map_err(sqlx_err)?;
    rows.iter().map(dataset_from_row).collect()
}
