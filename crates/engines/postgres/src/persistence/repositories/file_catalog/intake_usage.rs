// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use domain::storage::{DatasetIntakeUsage, OrganizationScope, PhysicalDatasetId};
use kernel::Result;
use sqlx::{PgPool, Row};

use super::{rows::to_u64, sqlx_err};

pub(super) async fn read(
    pool: &PgPool,
    scope: &OrganizationScope,
) -> Result<Vec<DatasetIntakeUsage>> {
    let rows = sqlx::query(
        // Bytes of cleaned-up flush records are folded into the dataset row; the total stays
        // unknown only while no record, kept or folded, knew its bytes.
        "SELECT dataset.id AS dataset_id,
                CASE WHEN SUM(flush.collected_bytes) IS NULL
                          AND dataset.folded_collected_bytes = 0 THEN NULL
                     ELSE (dataset.folded_collected_bytes
                           + COALESCE(SUM(flush.collected_bytes), 0))::BIGINT
                END AS collected_bytes,
                dataset.folded_collected_complete
                  AND COALESCE(BOOL_AND(flush.collected_bytes IS NOT NULL)
                                   FILTER (WHERE flush.flush_id IS NOT NULL), TRUE) AS complete
           FROM physical_datasets dataset
           LEFT JOIN storage_flush_commits flush
             ON flush.org_id = dataset.org_id AND flush.dataset_id = dataset.id
          WHERE dataset.org_id = $1
          GROUP BY dataset.id, dataset.folded_collected_bytes, dataset.folded_collected_complete
         HAVING COUNT(flush.flush_id) > 0 OR dataset.folded_collected_bytes > 0
             OR NOT dataset.folded_collected_complete",
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
