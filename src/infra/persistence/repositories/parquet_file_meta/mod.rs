// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::sync::Arc;

use async_trait::async_trait;
use object_store::ObjectStore;
use sqlx::{PgPool, Row, types::Json};

use super::{
    sqlx_err,
    streams::{stream_type_from_str, stream_type_to_str},
};
use crate::{
    domain::{
        storage::{
            ParquetFileMeta, ParquetFileMetaDumpRepository, ParquetFileMetaRepository,
            PhysicalDatasetKind,
        },
        stream::StreamType,
    },
    infra::caching::parquet_file_meta::dump::ParquetFileMetaDumpCacheRef,
    shared::{
        Error, Result,
        ids::Id,
        time::{TimeRange, TimestampMicros},
    },
};

mod cold;
pub mod dump;

/// `find()` 跨冷热边界合并 dump 时所需的依赖（spec `storage/ParquetFileMeta Dump Query Path`）。
///
/// `bootstrap` 在 bootstrap 阶段构造 `PgParquetFileMetaRepository` 后调
/// [`PgParquetFileMetaRepository::with_dump_query`] 注入；未注入时 `find` 行为退化到仅
/// 查主表，等价于 dump 未启用。
#[derive(Clone)]
pub struct DumpQueryContext {
    pub dump_repo: Arc<dyn ParquetFileMetaDumpRepository>,
    pub object_store: Arc<dyn ObjectStore>,
    pub cold_after_days: u32,
    /// 进程内 dump 缓存；`capacity = 0` 时为 noop wrapper，行为退化等价 None。
    /// 未注入时（None）跳过缓存查/写，每次 dump 命中都走 object_store。
    pub dump_cache: Option<ParquetFileMetaDumpCacheRef>,
}

pub struct PgParquetFileMetaRepository {
    pool: PgPool,
    dump_query: Option<DumpQueryContext>,
}

impl PgParquetFileMetaRepository {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            dump_query: None,
        }
    }

    pub fn with_dump_query(mut self, ctx: DumpQueryContext) -> Self {
        self.dump_query = Some(ctx);
        self
    }
}

const COLS: &str = "id, org_id, stream, stream_type, dataset_kind, object_key, time_start_micros,
                    time_end_micros, rows, size_bytes, min_values, max_values, deleted";

/// `find` 路径里 hot + cold 合并去重的内联辅助。**单独暴露成 free fn 让单元测试
/// 可直接覆盖** dedup / sort 语义，不需要 PG。
pub fn merge_hot_cold(
    mut hot: Vec<ParquetFileMeta>,
    mut cold: Vec<ParquetFileMeta>,
) -> Vec<ParquetFileMeta> {
    hot.append(&mut cold);
    hot.sort_by(|a, b| {
        a.time_range
            .start
            .0
            .cmp(&b.time_range.start.0)
            .then(a.id.0.cmp(&b.id.0))
    });
    hot.dedup_by(|a, b| a.id == b.id);
    hot
}

fn row_to_file(row: sqlx::postgres::PgRow) -> Result<ParquetFileMeta> {
    let stream_type: String = row.try_get("stream_type").map_err(sqlx_err)?;
    let min: Json<serde_json::Value> = row.try_get("min_values").map_err(sqlx_err)?;
    let max: Json<serde_json::Value> = row.try_get("max_values").map_err(sqlx_err)?;
    let min = match min.0 {
        serde_json::Value::Object(m) => m,
        other => {
            return Err(Error::internal(format!(
                "parquet_file_meta.min_values must be object, got {other:?}"
            )));
        }
    };
    let max = match max.0 {
        serde_json::Value::Object(m) => m,
        other => {
            return Err(Error::internal(format!(
                "parquet_file_meta.max_values must be object, got {other:?}"
            )));
        }
    };
    let rows: i64 = row.try_get("rows").map_err(sqlx_err)?;
    let size_bytes: i64 = row.try_get("size_bytes").map_err(sqlx_err)?;
    Ok(ParquetFileMeta {
        id: Id::from_string(row.try_get::<String, _>("id").map_err(sqlx_err)?),
        org_id: Id::from_string(row.try_get::<String, _>("org_id").map_err(sqlx_err)?),
        stream: row.try_get("stream").map_err(sqlx_err)?,
        stream_type: stream_type_from_str(&stream_type)?,
        dataset_kind: row
            .try_get::<String, _>("dataset_kind")
            .map_err(sqlx_err)?
            .parse()?,
        object_key: row.try_get("object_key").map_err(sqlx_err)?,
        time_range: TimeRange::new(
            TimestampMicros(row.try_get("time_start_micros").map_err(sqlx_err)?),
            TimestampMicros(row.try_get("time_end_micros").map_err(sqlx_err)?),
        ),
        rows: rows as u64,
        size_bytes: size_bytes as u64,
        min_values: min,
        max_values: max,
        deleted: row.try_get("deleted").map_err(sqlx_err)?,
    })
}

#[async_trait]
impl ParquetFileMetaRepository for PgParquetFileMetaRepository {
    async fn insert(&self, file: ParquetFileMeta) -> Result<()> {
        sqlx::query(
            "INSERT INTO parquet_file_meta
             (id, org_id, stream, stream_type, dataset_kind, object_key,
              time_start_micros, time_end_micros, rows, size_bytes,
              min_values, max_values, deleted)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)",
        )
        .bind(&file.id.0)
        .bind(&file.org_id.0)
        .bind(&file.stream)
        .bind(stream_type_to_str(file.stream_type))
        .bind(file.dataset_kind.as_str())
        .bind(&file.object_key)
        .bind(file.time_range.start.0)
        .bind(file.time_range.end.0)
        .bind(file.rows as i64)
        .bind(file.size_bytes as i64)
        .bind(Json(serde_json::Value::Object(file.min_values)))
        .bind(Json(serde_json::Value::Object(file.max_values)))
        .bind(file.deleted)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(())
    }

    async fn insert_many(&self, files: Vec<ParquetFileMeta>) -> Result<()> {
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;
        for file in files {
            sqlx::query(
                "INSERT INTO parquet_file_meta
                 (id, org_id, stream, stream_type, dataset_kind, object_key,
                  time_start_micros, time_end_micros, rows, size_bytes,
                  min_values, max_values, deleted)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)",
            )
            .bind(&file.id.0)
            .bind(&file.org_id.0)
            .bind(&file.stream)
            .bind(stream_type_to_str(file.stream_type))
            .bind(file.dataset_kind.as_str())
            .bind(&file.object_key)
            .bind(file.time_range.start.0)
            .bind(file.time_range.end.0)
            .bind(file.rows as i64)
            .bind(file.size_bytes as i64)
            .bind(Json(serde_json::Value::Object(file.min_values)))
            .bind(Json(serde_json::Value::Object(file.max_values)))
            .bind(file.deleted)
            .execute(&mut *tx)
            .await
            .map_err(sqlx_err)?;
        }
        tx.commit().await.map_err(sqlx_err)?;
        Ok(())
    }

    async fn find(
        &self,
        org_id: &Id,
        stream: &str,
        stream_type: StreamType,
        time_range: TimeRange,
    ) -> Result<Vec<ParquetFileMeta>> {
        let hot_rows = sqlx::query(&format!(
            "SELECT {COLS} FROM parquet_file_meta
             WHERE org_id = $1 AND stream = $2 AND stream_type = $3
               AND dataset_kind = 'raw'
               AND deleted = FALSE
               AND time_end_micros >= $4 AND time_start_micros < $5
             ORDER BY time_start_micros"
        ))
        .bind(&org_id.0)
        .bind(stream)
        .bind(stream_type_to_str(stream_type))
        .bind(time_range.start.0)
        .bind(time_range.end.0)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;
        let hot: Vec<ParquetFileMeta> = hot_rows
            .into_iter()
            .map(row_to_file)
            .collect::<Result<_>>()?;

        let Some(ctx) = self.dump_query.as_ref() else {
            return Ok(hot);
        };
        let cold = cold::load(
            ctx,
            org_id,
            stream,
            stream_type,
            PhysicalDatasetKind::Raw,
            time_range,
        )
        .await?;
        if cold.is_empty() {
            return Ok(hot);
        }
        Ok(merge_hot_cold(hot, cold))
    }

    async fn find_dataset(
        &self,
        org_id: &Id,
        stream: &str,
        stream_type: StreamType,
        dataset_kind: PhysicalDatasetKind,
        time_range: TimeRange,
    ) -> Result<Vec<ParquetFileMeta>> {
        if dataset_kind == PhysicalDatasetKind::Raw {
            return self.find(org_id, stream, stream_type, time_range).await;
        }
        let rows = sqlx::query(&format!(
            "SELECT {COLS} FROM parquet_file_meta
             WHERE org_id = $1 AND stream = $2 AND stream_type = $3
               AND dataset_kind = $4 AND deleted = FALSE
               AND time_end_micros >= $5 AND time_start_micros < $6
             ORDER BY time_start_micros"
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
        let hot = rows
            .into_iter()
            .map(row_to_file)
            .collect::<Result<Vec<_>>>()?;
        let Some(ctx) = self.dump_query.as_ref() else {
            return Ok(hot);
        };
        let cold = cold::load(ctx, org_id, stream, stream_type, dataset_kind, time_range).await?;
        Ok(merge_hot_cold(hot, cold))
    }

    #[tracing::instrument(
        name = "db.transaction",
        skip_all,
        fields(db.system.name = "postgresql", db.operation.name = "TRANSACTION", db.collection.name = "parquet_file_meta")
    )]
    async fn replace(&self, merged_ids: &[Id], new_files: Vec<ParquetFileMeta>) -> Result<()> {
        let mut tx = sqlx::begin(&self.pool).await.map_err(sqlx_err)?;

        if !merged_ids.is_empty() {
            let ids: Vec<String> = merged_ids.iter().map(|i| i.0.clone()).collect();
            let marked = sqlx::query(
                "UPDATE parquet_file_meta SET deleted = TRUE WHERE id = ANY($1) AND deleted = FALSE",
            )
            .bind(&ids)
            .execute(&mut *tx)
            .await
            .map_err(sqlx_err)?
            .rows_affected();
            // 标删数少于请求数 = 并发写者已经处理掉了其中一部分源文件。提前返回，
            // tx 在 drop 时回滚，新文件不会落库；调用方据此删掉自己刚写出的对象。
            if marked != merged_ids.len() as u64 {
                return Err(Error::conflict(format!(
                    "parquet_file_meta replace: {marked} of {} source files were live; \
                     another writer already compacted this group",
                    merged_ids.len()
                )));
            }
        }
        for f in new_files {
            sqlx::query(
                "INSERT INTO parquet_file_meta
                 (id, org_id, stream, stream_type, dataset_kind, object_key,
                  time_start_micros, time_end_micros, rows, size_bytes,
                  min_values, max_values, deleted)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)",
            )
            .bind(&f.id.0)
            .bind(&f.org_id.0)
            .bind(&f.stream)
            .bind(stream_type_to_str(f.stream_type))
            .bind(f.dataset_kind.as_str())
            .bind(&f.object_key)
            .bind(f.time_range.start.0)
            .bind(f.time_range.end.0)
            .bind(f.rows as i64)
            .bind(f.size_bytes as i64)
            .bind(Json(serde_json::Value::Object(f.min_values)))
            .bind(Json(serde_json::Value::Object(f.max_values)))
            .bind(f.deleted)
            .execute(&mut *tx)
            .await
            .map_err(sqlx_err)?;
        }

        tx.commit().await.map_err(sqlx_err)?;
        Ok(())
    }

    async fn mark_deleted(&self, ids: &[Id]) -> Result<usize> {
        if ids.is_empty() {
            return Ok(0);
        }
        let ids: Vec<String> = ids.iter().map(|i| i.0.clone()).collect();
        let marked = sqlx::query(
            "UPDATE parquet_file_meta SET deleted = TRUE WHERE id = ANY($1) AND deleted = FALSE",
        )
        .bind(&ids)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?
        .rows_affected();
        Ok(marked as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::time::TimestampMicros;

    fn fm(seed: u64, id: &str, start_us: i64) -> ParquetFileMeta {
        ParquetFileMeta {
            id: Id::from_string(id),
            org_id: Id::from_string("org-x"),
            stream: "app".into(),
            stream_type: StreamType::Logs,
            dataset_kind: PhysicalDatasetKind::Raw,
            object_key: format!("k/{seed}.parquet"),
            time_range: TimeRange::new(TimestampMicros(start_us), TimestampMicros(start_us + 1000)),
            rows: 1,
            size_bytes: 64,
            min_values: serde_json::Map::new(),
            max_values: serde_json::Map::new(),
            deleted: false,
        }
    }

    #[test]
    fn merge_hot_cold_dedups_by_id_and_sorts_by_time_start() {
        // Cold-only id "a"，hot-only id "b"，同 id "c" 在 hot 与 cold 各出现一次。
        let hot = vec![fm(1, "b", 2000), fm(2, "c", 3000)];
        let cold = vec![fm(3, "a", 1000), fm(4, "c", 3000)];
        let merged = merge_hot_cold(hot, cold);
        assert_eq!(merged.len(), 3, "duplicate id c collapsed to a single row");
        // 按 time_range.start 升序：a(1000) → b(2000) → c(3000)
        assert_eq!(merged[0].id.0, "a");
        assert_eq!(merged[1].id.0, "b");
        assert_eq!(merged[2].id.0, "c");
    }

    #[test]
    fn merge_hot_cold_handles_empty_inputs() {
        assert!(merge_hot_cold(vec![], vec![]).is_empty());
        let only_hot = merge_hot_cold(vec![fm(1, "x", 100)], vec![]);
        assert_eq!(only_hot.len(), 1);
        let only_cold = merge_hot_cold(vec![], vec![fm(1, "x", 100)]);
        assert_eq!(only_cold.len(), 1);
    }
}
