// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `wal_writers` and `wal_streams`: writer epochs, their leases, and who may publish each stream.
//!
//! Times are the database's own (`now()`), so the leases of all nodes are judged by one clock.
//! Fencing, renewal and claims are single conditional updates: of two that race on one row, the
//! second sees the first's result. `FileCatalog::commit_flush` reads the same rows under a share
//! lock, so a fence or claim waits for a publication in flight and every later one sees it.

use std::time::Duration;

use async_trait::async_trait;
use domain::storage::{
    DatasetTypeId, OrphanedStreams, PhysicalDatasetId, WalStreamRow, WalWriterRegistry,
    WriterEpoch, WriterNodeId,
};
use kernel::{Error, Result, ids::Id};
use sqlx::{PgPool, Row, postgres::PgRow};

use super::sqlx_err;

pub struct PgWalWriterRegistry {
    pool: PgPool,
}

impl PgWalWriterRegistry {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn millis(lease: Duration) -> i64 {
    i64::try_from(lease.as_millis()).unwrap_or(i64::MAX)
}

fn epoch_param(epoch: WriterEpoch) -> Result<i64> {
    i64::try_from(epoch.0).map_err(|_| Error::invalid(format!("writer epoch {epoch} too large")))
}

fn epoch_column(row: &PgRow, column: &str) -> Result<WriterEpoch> {
    let value: i64 = row.try_get(column).map_err(sqlx_err)?;

    u64::try_from(value)
        .map(WriterEpoch)
        .map_err(|_| Error::internal(format!("negative {column} {value}")))
}

fn stream_row(row: &PgRow) -> Result<WalStreamRow> {
    let dataset_type: String = row.try_get("dataset_type").map_err(sqlx_err)?;

    Ok(WalStreamRow {
        organization_id: Id::from_string(row.try_get::<String, _>("org_id").map_err(sqlx_err)?),
        dataset_id: PhysicalDatasetId::from_string(
            row.try_get::<String, _>("dataset_id").map_err(sqlx_err)?,
        ),
        dataset_type: DatasetTypeId::new(&dataset_type)
            .map_err(|error| Error::internal(format!("wal stream dataset type: {error}")))?,
        writer_node_id: WriterNodeId::new(
            row.try_get::<String, _>("writer_node_id")
                .map_err(sqlx_err)?,
        ),
        writer_epoch: epoch_column(row, "writer_epoch")?,
        publisher_epoch: epoch_column(row, "publisher_epoch")?,
    })
}

#[async_trait]
impl WalWriterRegistry for PgWalWriterRegistry {
    async fn register(&self, node_id: &WriterNodeId, lease: Duration) -> Result<WriterEpoch> {
        let row = sqlx::query(
            "INSERT INTO wal_writers (writer_node_id, started_at, lease_expires_at)
             VALUES ($1, now(), now() + $2 * interval '1 millisecond')
             RETURNING writer_epoch",
        )
        .bind(node_id.as_str())
        .bind(millis(lease))
        .fetch_one(&self.pool)
        .await
        .map_err(sqlx_err)?;

        epoch_column(&row, "writer_epoch")
    }

    async fn renew(&self, epoch: WriterEpoch, lease: Duration) -> Result<bool> {
        let renewed = sqlx::query(
            "UPDATE wal_writers SET lease_expires_at = now() + $2 * interval '1 millisecond'
              WHERE writer_epoch = $1 AND fenced_at IS NULL",
        )
        .bind(epoch_param(epoch)?)
        .bind(millis(lease))
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?
        .rows_affected();

        Ok(renewed == 1)
    }

    async fn release(&self, epoch: WriterEpoch) -> Result<()> {
        sqlx::query(
            "UPDATE wal_writers
                SET fenced_at = COALESCE(fenced_at, now()),
                    lease_expires_at = LEAST(lease_expires_at, now())
              WHERE writer_epoch = $1",
        )
        .bind(epoch_param(epoch)?)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(())
    }

    async fn open_stream(&self, stream: &WalStreamRow) -> Result<()> {
        sqlx::query(
            "INSERT INTO wal_streams (org_id, dataset_id, writer_epoch, writer_node_id,
                                      dataset_type, publisher_epoch, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, now())
             ON CONFLICT (org_id, dataset_id, writer_epoch) DO NOTHING",
        )
        .bind(stream.organization_id.as_str())
        .bind(stream.dataset_id.as_str())
        .bind(epoch_param(stream.writer_epoch)?)
        .bind(stream.writer_node_id.as_str())
        .bind(stream.dataset_type.as_str())
        .bind(epoch_param(stream.publisher_epoch)?)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;

        Ok(())
    }

    async fn close_stream(
        &self,
        organization_id: &Id,
        dataset_id: &PhysicalDatasetId,
        writer_epoch: WriterEpoch,
        publisher_epoch: WriterEpoch,
    ) -> Result<bool> {
        let removed = sqlx::query(
            "DELETE FROM wal_streams
              WHERE org_id = $1 AND dataset_id = $2 AND writer_epoch = $3
                AND publisher_epoch = $4",
        )
        .bind(organization_id.as_str())
        .bind(dataset_id.as_str())
        .bind(epoch_param(writer_epoch)?)
        .bind(epoch_param(publisher_epoch)?)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?
        .rows_affected();

        Ok(removed == 1)
    }

    async fn fence_expired(&self) -> Result<u64> {
        let fenced = sqlx::query(
            "UPDATE wal_writers SET fenced_at = now()
              WHERE fenced_at IS NULL AND lease_expires_at < now()",
        )
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?
        .rows_affected();

        Ok(fenced)
    }

    async fn orphaned_streams(
        &self,
        claimant: WriterEpoch,
        kind: &OrphanedStreams,
        limit: u32,
    ) -> Result<Vec<WalStreamRow>> {
        let (dataset_type, only) = match kind {
            OrphanedStreams::Except(dataset_type) => (dataset_type, false),
            OrphanedStreams::Only(dataset_type) => (dataset_type, true),
        };

        let rows = sqlx::query(
            "SELECT s.org_id, s.dataset_id, s.writer_epoch, s.writer_node_id, s.dataset_type,
                    s.publisher_epoch
               FROM wal_streams s
               JOIN wal_writers publisher ON publisher.writer_epoch = s.publisher_epoch
              WHERE (publisher.fenced_at IS NOT NULL
                     OR (s.publisher_epoch = $2 AND s.writer_epoch <> $2))
                AND (s.dataset_type = $3) = $4
              ORDER BY s.writer_epoch, s.org_id, s.dataset_id
              LIMIT $1",
        )
        .bind(i64::from(limit))
        .bind(epoch_param(claimant)?)
        .bind(dataset_type.as_str())
        .bind(only)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        rows.iter().map(stream_row).collect()
    }

    async fn claim_stream(&self, stream: &WalStreamRow, claimant: WriterEpoch) -> Result<bool> {
        let claimed = sqlx::query(
            "UPDATE wal_streams s SET publisher_epoch = $4
              WHERE s.org_id = $1 AND s.dataset_id = $2 AND s.writer_epoch = $3
                AND (s.publisher_epoch = $4
                     OR EXISTS (SELECT 1 FROM wal_writers current
                                 WHERE current.writer_epoch = s.publisher_epoch
                                   AND current.fenced_at IS NOT NULL))
                AND EXISTS (SELECT 1 FROM wal_writers claimant
                             WHERE claimant.writer_epoch = $4 AND claimant.fenced_at IS NULL)",
        )
        .bind(stream.organization_id.as_str())
        .bind(stream.dataset_id.as_str())
        .bind(epoch_param(stream.writer_epoch)?)
        .bind(epoch_param(claimant)?)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?
        .rows_affected();

        Ok(claimed == 1)
    }

    async fn forget_finished_writers(&self) -> Result<u64> {
        let forgotten = sqlx::query(
            "DELETE FROM wal_writers w
              WHERE w.fenced_at IS NOT NULL
                AND NOT EXISTS (SELECT 1 FROM wal_streams s WHERE s.writer_epoch = w.writer_epoch)
                AND NOT EXISTS (SELECT 1 FROM wal_streams s
                                 WHERE s.publisher_epoch = w.writer_epoch)",
        )
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?
        .rows_affected();

        Ok(forgotten)
    }
}
