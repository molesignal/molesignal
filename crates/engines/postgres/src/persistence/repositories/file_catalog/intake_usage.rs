// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use sqlx::{PgPool, Row};

use super::{rows::to_u64, sqlx_err};
use crate::{
    domain::storage::{DatasetIntakeUsage, OrganizationScope, PhysicalDatasetId},
    shared::Result,
};

pub(super) async fn read(
    pool: &PgPool,
    scope: &OrganizationScope,
) -> Result<Vec<DatasetIntakeUsage>> {
    let rows = sqlx::query(
        "SELECT dataset_id, SUM(collected_bytes)::BIGINT AS collected_bytes,
                BOOL_AND(collected_bytes IS NOT NULL) AS complete
         FROM storage_flush_commits WHERE org_id = $1 GROUP BY dataset_id",
    )
    .bind(scope.organization_id.as_str())
    .fetch_all(pool)
    .await
    .map_err(sqlx_err)?;
    rows.into_iter()
        .map(|row| {
            let bytes: Option<i64> = row.try_get("collected_bytes").map_err(sqlx_err)?;
            Ok(DatasetIntakeUsage {
                dataset_id: PhysicalDatasetId::from_string(
                    row.try_get::<String, _>("dataset_id").map_err(sqlx_err)?,
                ),
                collected_bytes: bytes
                    .map(|bytes| to_u64(bytes, "collected_bytes"))
                    .transpose()?,
                complete: row.try_get("complete").map_err(sqlx_err)?,
            })
        })
        .collect()
}
