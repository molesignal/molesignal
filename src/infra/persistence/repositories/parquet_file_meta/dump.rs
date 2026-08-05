// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `parquet_file_meta_dump` Postgres 实装（spec `storage/ParquetFileMeta Dump Spillover`）。
//!
//! 表是 dump 索引：每行一条 `(org, stream, stream_type, dataset_kind,
//! partition_level, partition_key)` 分区指针，指向 object_store 上一份
//! columnar dump.parquet。
//! 不存原始 ParquetFileMeta 字段，只存 PG 侧裁剪所需最小元信息（`min/max_ts_micros`
//! + `deleted` + `size_bytes` + `updated_at_micros`）。
//!
//! 与 `parquet_file_meta_dump_stats` 表是 1:1 FK（CASCADE）；任何 dump 行写入必须同时
//! 写入 stats 行，删除/重写时同步。
//!
//! change `parquet-file-meta-dump-columnar`：旧的单列 JSON parquet + 仅
//! `(org, stream, stream_type, date)` 主键 schema 已废弃。

use async_trait::async_trait;
use sqlx::{PgPool, Row};

use super::super::{
    sqlx_err,
    streams::{stream_type_from_str, stream_type_to_str},
};
use crate::{
    domain::{
        storage::{
            ParquetFileMetaDumpRepository, ParquetFileMetaDumpRow, ParquetFileMetaDumpStats,
            PartitionLevel, PhysicalDatasetKind,
        },
        stream::StreamType,
    },
    shared::{
        Error, Result,
        ids::Id,
        time::{TimeRange, TimestampMicros},
    },
};

pub struct PgParquetFileMetaDumpRepository {
    pool: PgPool,
}

impl PgParquetFileMetaDumpRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }
}

const COLS: &str = "id, org_id, stream, stream_type, dataset_kind, partition_level, partition_key, object_key, \
    deleted, rows_in_dump, size_bytes, min_ts_micros, max_ts_micros, date, \
    created_at_micros, updated_at_micros";

const STATS_COLS: &str = "object_key, rows_total, files_total, time_start_micros, \
    time_end_micros, storage_size_bytes, updated_at_micros";

fn partition_level_from_str(s: &str) -> Result<PartitionLevel> {
    match s {
        "daily" => Ok(PartitionLevel::Daily),
        "hourly" => Ok(PartitionLevel::Hourly),
        other => Err(Error::internal(format!("unknown partition_level: {other}"))),
    }
}

fn row_to_dump(row: sqlx::postgres::PgRow) -> Result<ParquetFileMetaDumpRow> {
    let stream_type: String = row.try_get("stream_type").map_err(sqlx_err)?;
    let date: chrono::NaiveDate = row.try_get("date").map_err(sqlx_err)?;
    let partition_level: String = row.try_get("partition_level").map_err(sqlx_err)?;
    let rows_in_dump: i32 = row.try_get("rows_in_dump").map_err(sqlx_err)?;
    Ok(ParquetFileMetaDumpRow {
        id: Id::from_string(row.try_get::<String, _>("id").map_err(sqlx_err)?),
        org_id: Id::from_string(row.try_get::<String, _>("org_id").map_err(sqlx_err)?),
        stream: row.try_get("stream").map_err(sqlx_err)?,
        stream_type: stream_type_from_str(&stream_type)?,
        dataset_kind: row
            .try_get::<String, _>("dataset_kind")
            .map_err(sqlx_err)?
            .parse()?,
        date: date.format("%Y-%m-%d").to_string(),
        object_key: row.try_get("object_key").map_err(sqlx_err)?,
        rows_in_dump: rows_in_dump as u32,
        created_at: TimestampMicros(row.try_get("created_at_micros").map_err(sqlx_err)?),
        partition_level: partition_level_from_str(&partition_level)?,
        partition_key: row.try_get("partition_key").map_err(sqlx_err)?,
        deleted: row.try_get("deleted").map_err(sqlx_err)?,
        min_ts_micros: row.try_get("min_ts_micros").map_err(sqlx_err)?,
        max_ts_micros: row.try_get("max_ts_micros").map_err(sqlx_err)?,
        size_bytes: row.try_get("size_bytes").map_err(sqlx_err)?,
        updated_at_micros: row.try_get("updated_at_micros").map_err(sqlx_err)?,
    })
}

fn row_to_stats(row: sqlx::postgres::PgRow) -> Result<ParquetFileMetaDumpStats> {
    Ok(ParquetFileMetaDumpStats {
        object_key: row.try_get("object_key").map_err(sqlx_err)?,
        rows_total: row.try_get("rows_total").map_err(sqlx_err)?,
        files_total: row.try_get("files_total").map_err(sqlx_err)?,
        time_start_micros: row.try_get("time_start_micros").map_err(sqlx_err)?,
        time_end_micros: row.try_get("time_end_micros").map_err(sqlx_err)?,
        storage_size_bytes: row.try_get("storage_size_bytes").map_err(sqlx_err)?,
        updated_at_micros: row.try_get("updated_at_micros").map_err(sqlx_err)?,
    })
}

fn date_from_partition_key(partition_key: &str) -> Result<chrono::NaiveDate> {
    // daily → "YYYY-MM-DD"; hourly → "YYYY-MM-DD-HH" (we strip the trailing hour).
    let date_part = partition_key
        .get(0..10)
        .ok_or_else(|| Error::invalid(format!("partition_key too short: '{partition_key}'")))?;
    chrono::NaiveDate::parse_from_str(date_part, "%Y-%m-%d")
        .map_err(|e| Error::invalid(format!("invalid partition_key '{partition_key}': {e}")))
}

#[async_trait]
impl ParquetFileMetaDumpRepository for PgParquetFileMetaDumpRepository {
    async fn insert(&self, row: ParquetFileMetaDumpRow) -> Result<()> {
        // Backwards-compat wrapper for legacy callers (none in current tree).
        // upsert_dump is the canonical path; we synthesize a stats row from the
        // fields already on `row`.
        let stats = ParquetFileMetaDumpStats {
            object_key: row.object_key.clone(),
            rows_total: i64::from(row.rows_in_dump),
            files_total: i64::from(row.rows_in_dump),
            time_start_micros: row.min_ts_micros,
            time_end_micros: row.max_ts_micros,
            storage_size_bytes: row.size_bytes,
            updated_at_micros: row.updated_at_micros.max(row.created_at.0),
        };
        self.upsert_dump(row, stats).await
    }

    async fn find_by_time_range(
        &self,
        org_id: &Id,
        stream: &str,
        stream_type: StreamType,
        dataset_kind: PhysicalDatasetKind,
        time_range: TimeRange,
    ) -> Result<Vec<ParquetFileMetaDumpRow>> {
        let rows = sqlx::query(&format!(
            "SELECT {COLS} FROM parquet_file_meta_dump
             WHERE org_id = $1 AND stream = $2 AND stream_type = $3
               AND dataset_kind = $4
               AND deleted = FALSE
               AND max_ts_micros >= $5
               AND min_ts_micros < $6
             ORDER BY min_ts_micros"
        ))
        .bind(&org_id.0)
        .bind(stream)
        .bind(stream_type_to_str(stream_type))
        .bind(dataset_kind.as_str())
        .bind(time_range.start.0)
        .bind(time_range.end.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;
        rows.into_iter().map(row_to_dump).collect()
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        sqlx::query("DELETE FROM parquet_file_meta_dump WHERE id = $1")
            .bind(&id.0)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;
        Ok(())
    }

    async fn mark_deleted(&self, object_key: &str) -> Result<()> {
        let now = TimestampMicros::now().0;
        sqlx::query(
            "UPDATE parquet_file_meta_dump
             SET deleted = TRUE, updated_at_micros = $2
             WHERE object_key = $1",
        )
        .bind(object_key)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(())
    }

    #[tracing::instrument(
        name = "db.transaction",
        skip_all,
        fields(db.system.name = "postgresql", db.operation.name = "TRANSACTION", db.collection.name = "parquet_file_meta_dump")
    )]
    async fn upsert_dump(
        &self,
        row: ParquetFileMetaDumpRow,
        stats: ParquetFileMetaDumpStats,
    ) -> Result<()> {
        if row.object_key != stats.object_key {
            return Err(Error::invalid(format!(
                "upsert_dump: row.object_key='{}' but stats.object_key='{}'",
                row.object_key, stats.object_key
            )));
        }
        let date = date_from_partition_key(&row.partition_key)?;
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;

        // dump 行：按 (org, stream, stream_type, dataset_kind, partition_level, partition_key, deleted=FALSE)
        // 的部分唯一索引兜底冲突；如果同 partition 已有 live 行则覆盖。
        sqlx::query(
            "INSERT INTO parquet_file_meta_dump
             (id, org_id, stream, stream_type, dataset_kind, partition_level, partition_key,
              object_key, deleted, rows_in_dump, size_bytes,
              min_ts_micros, max_ts_micros, date,
              created_at_micros, updated_at_micros)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, FALSE, $9, $10, $11, $12, $13, $14, $15)
             ON CONFLICT (org_id, stream, stream_type, dataset_kind, partition_level, partition_key)
               WHERE deleted = FALSE
             DO UPDATE SET
                 object_key        = EXCLUDED.object_key,
                 rows_in_dump      = EXCLUDED.rows_in_dump,
                 size_bytes        = EXCLUDED.size_bytes,
                 min_ts_micros     = EXCLUDED.min_ts_micros,
                 max_ts_micros     = EXCLUDED.max_ts_micros,
                 date              = EXCLUDED.date,
                 updated_at_micros = EXCLUDED.updated_at_micros",
        )
        .bind(&row.id.0)
        .bind(&row.org_id.0)
        .bind(&row.stream)
        .bind(stream_type_to_str(row.stream_type))
        .bind(row.dataset_kind.as_str())
        .bind(row.partition_level.as_str())
        .bind(&row.partition_key)
        .bind(&row.object_key)
        .bind(row.rows_in_dump as i32)
        .bind(row.size_bytes)
        .bind(row.min_ts_micros)
        .bind(row.max_ts_micros)
        .bind(date)
        .bind(row.created_at.0)
        .bind(row.updated_at_micros)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?;

        // stats 行：FK references dump.object_key so the dump INSERT must commit
        // first within the same tx — sqlx serializes by &mut *tx.
        sqlx::query(
            "INSERT INTO parquet_file_meta_dump_stats
             (object_key, rows_total, files_total,
              time_start_micros, time_end_micros,
              storage_size_bytes, updated_at_micros)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             ON CONFLICT (object_key) DO UPDATE SET
                 rows_total         = EXCLUDED.rows_total,
                 files_total        = EXCLUDED.files_total,
                 time_start_micros  = EXCLUDED.time_start_micros,
                 time_end_micros    = EXCLUDED.time_end_micros,
                 storage_size_bytes = EXCLUDED.storage_size_bytes,
                 updated_at_micros  = EXCLUDED.updated_at_micros",
        )
        .bind(&stats.object_key)
        .bind(stats.rows_total)
        .bind(stats.files_total)
        .bind(stats.time_start_micros)
        .bind(stats.time_end_micros)
        .bind(stats.storage_size_bytes)
        .bind(stats.updated_at_micros)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?;

        tx.commit().await.map_err(sqlx_err)?;
        Ok(())
    }

    #[tracing::instrument(
        name = "db.transaction",
        skip_all,
        fields(db.system.name = "postgresql", db.operation.name = "TRANSACTION", db.collection.name = "parquet_file_meta_dump")
    )]
    async fn insert_rewrite(
        &self,
        old_object_key: &str,
        new_row: ParquetFileMetaDumpRow,
        new_stats: ParquetFileMetaDumpStats,
    ) -> Result<()> {
        if new_row.object_key != new_stats.object_key {
            return Err(Error::invalid(format!(
                "insert_rewrite: new_row.object_key='{}' but new_stats.object_key='{}'",
                new_row.object_key, new_stats.object_key
            )));
        }
        if new_row.object_key == old_object_key {
            return Err(Error::invalid(format!(
                "insert_rewrite: new_object_key must differ from old_object_key '{old_object_key}'"
            )));
        }
        let date = date_from_partition_key(&new_row.partition_key)?;
        let now = TimestampMicros::now().0;
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;

        // Step 1: mark old row deleted (drops partial-unique-index seat → frees
        // it for the new live row).
        sqlx::query(
            "UPDATE parquet_file_meta_dump
             SET deleted = TRUE, updated_at_micros = $2
             WHERE object_key = $1",
        )
        .bind(old_object_key)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?;

        // Step 2: drop old stats row (CASCADE would do it on dump-row DELETE,
        // but we keep the dump row for audit; stats row tied to live dump only).
        sqlx::query("DELETE FROM parquet_file_meta_dump_stats WHERE object_key = $1")
            .bind(old_object_key)
            .execute(&mut *tx)
            .await
            .map_err(sqlx_err)?;

        // Step 3: insert new live dump row.
        sqlx::query(
            "INSERT INTO parquet_file_meta_dump
             (id, org_id, stream, stream_type, dataset_kind, partition_level, partition_key,
              object_key, deleted, rows_in_dump, size_bytes,
              min_ts_micros, max_ts_micros, date,
              created_at_micros, updated_at_micros)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, FALSE, $9, $10, $11, $12, $13, $14, $15)",
        )
        .bind(&new_row.id.0)
        .bind(&new_row.org_id.0)
        .bind(&new_row.stream)
        .bind(stream_type_to_str(new_row.stream_type))
        .bind(new_row.dataset_kind.as_str())
        .bind(new_row.partition_level.as_str())
        .bind(&new_row.partition_key)
        .bind(&new_row.object_key)
        .bind(new_row.rows_in_dump as i32)
        .bind(new_row.size_bytes)
        .bind(new_row.min_ts_micros)
        .bind(new_row.max_ts_micros)
        .bind(date)
        .bind(new_row.created_at.0)
        .bind(new_row.updated_at_micros)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?;

        // Step 4: insert new stats row.
        sqlx::query(
            "INSERT INTO parquet_file_meta_dump_stats
             (object_key, rows_total, files_total,
              time_start_micros, time_end_micros,
              storage_size_bytes, updated_at_micros)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(&new_stats.object_key)
        .bind(new_stats.rows_total)
        .bind(new_stats.files_total)
        .bind(new_stats.time_start_micros)
        .bind(new_stats.time_end_micros)
        .bind(new_stats.storage_size_bytes)
        .bind(new_stats.updated_at_micros)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?;

        tx.commit().await.map_err(sqlx_err)?;
        Ok(())
    }

    async fn pending_object_deletes(&self, limit: u32) -> Result<Vec<ParquetFileMetaDumpRow>> {
        let rows = sqlx::query(&format!(
            "SELECT {COLS} FROM parquet_file_meta_dump
             WHERE deleted = TRUE
             ORDER BY updated_at_micros
             LIMIT $1"
        ))
        .bind(i64::from(limit))
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;
        rows.into_iter().map(row_to_dump).collect()
    }
}

impl PgParquetFileMetaDumpRepository {
    /// stream stats consumer 路径用：按 `(org, stream, stream_type, time_range)`
    /// 一次性 SUM 出 dump-tier 聚合。不打开任何 parquet。
    pub async fn aggregate_stats_in_range(
        &self,
        org_id: &Id,
        stream: &str,
        stream_type: StreamType,
        time_range: TimeRange,
    ) -> Result<ParquetFileMetaDumpStats> {
        let row = sqlx::query(
            "SELECT
                COALESCE(SUM(s.rows_total), 0)::BIGINT          AS rows_total,
                COALESCE(SUM(s.files_total), 0)::BIGINT         AS files_total,
                COALESCE(MIN(s.time_start_micros), 0)::BIGINT   AS time_start_micros,
                COALESCE(MAX(s.time_end_micros), 0)::BIGINT     AS time_end_micros,
                COALESCE(SUM(s.storage_size_bytes), 0)::BIGINT  AS storage_size_bytes
             FROM parquet_file_meta_dump_stats s
             JOIN parquet_file_meta_dump d ON d.object_key = s.object_key
             WHERE d.org_id = $1 AND d.stream = $2 AND d.stream_type = $3
               AND d.dataset_kind = 'raw'
               AND d.deleted = FALSE
               AND d.max_ts_micros >= $4
               AND d.min_ts_micros < $5",
        )
        .bind(&org_id.0)
        .bind(stream)
        .bind(stream_type_to_str(stream_type))
        .bind(time_range.start.0)
        .bind(time_range.end.0)
        .fetch_one(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(ParquetFileMetaDumpStats {
            object_key: String::new(),
            rows_total: row.try_get("rows_total").map_err(sqlx_err)?,
            files_total: row.try_get("files_total").map_err(sqlx_err)?,
            time_start_micros: row.try_get("time_start_micros").map_err(sqlx_err)?,
            time_end_micros: row.try_get("time_end_micros").map_err(sqlx_err)?,
            storage_size_bytes: row.try_get("storage_size_bytes").map_err(sqlx_err)?,
            updated_at_micros: TimestampMicros::now().0,
        })
    }

    /// service 路径直接调用：按 object_key 拿 dump 行 + stats 行的内容。
    pub async fn list_live_in_range(
        &self,
        org_id: &Id,
        stream: &str,
        stream_type: StreamType,
        time_range: TimeRange,
    ) -> Result<Vec<ParquetFileMetaDumpRow>> {
        self.find_by_time_range(
            org_id,
            stream,
            stream_type,
            PhysicalDatasetKind::Raw,
            time_range,
        )
        .await
    }

    pub async fn find_stats_by_object_key(
        &self,
        object_key: &str,
    ) -> Result<Option<ParquetFileMetaDumpStats>> {
        let row_opt = sqlx::query(&format!(
            "SELECT {STATS_COLS} FROM parquet_file_meta_dump_stats WHERE object_key = $1"
        ))
        .bind(object_key)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?;
        row_opt.map(row_to_stats).transpose()
    }
}
