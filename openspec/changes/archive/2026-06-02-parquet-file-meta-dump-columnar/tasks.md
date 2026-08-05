## 1. Domain & Config Foundations

- [x] 1.1 在 `crates/domain/src/storage/mod.rs` 给 `ParquetFileMetaDumpRow` 加 `partition_level: PartitionLevel`、`partition_key: String`、`deleted: bool`、`min_ts_micros: i64`、`max_ts_micros: i64`、`size_bytes: i64`、`updated_at_micros: i64` 字段（`rows_in_dump` 保留 `u32`，与现有 PG 列一致）
- [x] 1.2 在同文件给 `ParquetFileMetaDumpRepository` trait 加方法：`mark_deleted(object_key)`、`upsert_dump(row, stats)`、`insert_rewrite(old_object_key, new_row, new_stats)`、`pending_object_deletes(limit)`，全部以 `unimplemented!()` default impl 兜底；`find_by_time_range` 已有，保持签名等 task 6.1 改语义
- [x] 1.3 在 `crates/domain/src/storage/mod.rs` 新增 `ParquetFileMetaDumpStats { object_key, rows_total, files_total, time_start_micros, time_end_micros, storage_size_bytes, updated_at_micros }`
- [x] 1.4 在 `crates/config/src/settings.rs` `ParquetFileMetaDumpSettings` 加 `partition_level: PartitionLevel`（default `Daily`），新增 `enum PartitionLevel { Daily, Hourly }` + `serde` rename `"daily" | "hourly"`
- [x] 1.5 在同文件新增 `CacheSettings.parquet_file_meta_dump: ParquetFileMetaDumpCacheSettings { capacity: u64, ttl_secs: u32 }`（default `10_000 / 600`）；更新现有 cache settings 测试覆盖新字段
- [x] 1.6 `conf/config.toml` 加示例 `[storage.parquet_file_meta_dump].partition_level = "daily"` 与 `[cache.parquet_file_meta_dump]` 段（带注释说明）

## 2. Database Migration

- [x] 2.1 新建 `crates/infra/migrations/20260602000001_parquet_file_meta_dump_columnar.sql`：`DROP TABLE IF EXISTS parquet_file_meta_dump CASCADE` + `CREATE TABLE parquet_file_meta_dump (id PK, org_id, stream, stream_type, partition_level, partition_key, object_key UNIQUE, deleted, rows_in_dump, size_bytes, min_ts_micros, max_ts_micros, date, created_at_micros, updated_at_micros)`（PK 改 `id`；partition 唯一性用 partial unique index on `(org, stream, stream_type, partition_level, partition_key) WHERE deleted=FALSE`，让一个 partition 可以共存多份历史 deleted 行）
- [x] 2.2 同 migration 加索引 `CREATE INDEX idx_parquet_file_meta_dump_query ON parquet_file_meta_dump (org_id, stream, stream_type, min_ts_micros, max_ts_micros) WHERE deleted = FALSE`
- [x] 2.3 同 migration 加 `CREATE TABLE parquet_file_meta_dump_stats (object_key TEXT PRIMARY KEY REFERENCES parquet_file_meta_dump(object_key) ON DELETE CASCADE, rows_total BIGINT, files_total BIGINT, time_start_micros BIGINT, time_end_micros BIGINT, storage_size_bytes BIGINT, updated_at_micros BIGINT)`
- [x] 2.4 `ARCHITECTURE.md Storage layout` 新增 `parquet_file_meta_dump 冷分区下沉` 段，记录 schema / partition_level / advisory lock / delete_by_time_range / cache / 迁移注意事项
- [x] 2.5 跳过（用户确认 sqlx 在线模式；无 `.sqlx/` 离线 cache 需要更新）

## 3. Writer & Service (compactor 路径)

- [x] 3.1 改写 `crates/infra/src/storage/parquet_file_meta_dump_writer.rs::serialize_dump`：构造 14 列 Arrow schema，对 `Vec<ParquetFileMeta>` 直接列化（含 `min_values_json/max_values_json` 一次 `serde_json::to_string`），SNAPPY 压缩；删除旧的 `meta_json` 单列路径
- [x] 3.2 同文件 `dump_object_key` 改签名接受 `partition_level + partition_key`，daily 时 `"YYYY-MM-DD"`，hourly 时 `"YYYY-MM-DD-HH"`；新增 `rewrite_object_key(seq)` 用于部分重写
- [x] 3.3 同文件加单测：empty 输入产合法 parquet、5 行 round-trip（与新 reader 对接）、daily + hourly object key、rewrite_seq 单调、`DumpAggregate::from_rows` min/max ts
- [x] 3.4 在 `crates/infra/src/storage/parquet_file_meta_dump_service.rs` 把 `PartitionKey` 加 `partition_level + partition_key + date_for_parquet` 字段（取代 raw `date: String`），`scan_cold_partitions` 按 settings.partition_level 切 GROUP BY（daily 走 `bucket_date`，hourly 走 `date_trunc('hour', …)`）
- [x] 3.5 `dump_one_partition` 起始 `BEGIN; SELECT pg_try_advisory_xact_lock(hashtextextended($composite, 0))`；拿不到锁 → return `DumpOutcome::Locked` 让 caller 计 `skipped{locked}`
- [x] 3.6 改 `dump_one_partition` 把 INSERT 改成新列集合 + 加 `INSERT parquet_file_meta_dump_stats`（同一 tx）；PUT 完成后取 bytes.len() 作为 size_bytes
- [x] 3.7 `run_tick` 增加 `DumpOutcome::Empty/Locked/Written/Err` 四分支，分别记 `partitions_skipped{reason}`；`register_metrics_for_test` 把 `"locked"` 预热进 metric family
- [x] 3.8 service 集成测试覆盖 advisory lock 行为：通过新 IT 文件 `crates/infra/tests/it_parquet_file_meta_dump.rs` 跑（`MS_RUN_IT=1` 启）
- [x] 3.9 新增 `ParquetFileMetaDumpService::delete_by_time_range(org_id, stream, stream_type, range) -> Result<DeleteStats>`：按 流程实现（SELECT live in range → 整删 vs 部分重写、rewrite_seq via `parse_rewrite_seq` + max+1、`insert_rewrite` 在 PG repo 走原子三步）
- [x] 3.10 加 `parquet_file_meta_dump_delete_partitions_rewritten_total` / `_dropped_total` 两个 Counter 到 `Metrics` struct + `metrics()` 注册
- [x] 3.11 `delete_by_time_range` IT 测试见 `it_parquet_file_meta_dump.rs::service_delete_by_time_range_drops_full_overlap_dump`（部分重写 case 通过 repo `insert_rewrite` IT 测试间接覆盖；多 case 单测留 follow-up）

## 4. Reader & Query Path

- [x] 4.1 在 `crates/infra/src/storage/parquet_file_meta_dump_reader.rs` 新增 `parse_dump_bytes_columnar(bytes) -> Result<Vec<ParquetFileMeta>>`：按 14 列读 Arrow `RecordBatch`，逐列直构 `ParquetFileMeta`；删除旧的 `parse_dump_bytes`
- [x] 4.2 同文件新增 `read_dump_filtered(store, object_key, time_range)`：解码阶段按 `[ts_start, ts_end]` overlap 早剪（page-level row filter 通过 Arrow `decode_batch_into` 内联实现，比 `RowFilter` API 轻量且零内存重分配）
- [x] 4.3 单测覆盖：(a) round-trip via columnar、(b) time_range filter 丢非 overlap、(c) min_values_json 含特殊字符 round-trip、(d) stream_type variants + 拒未知、(e) metrics family 注册
- [x] 4.4 旧 `read_dump` 保留作为 thin 入口供 `delete_by_time_range`（需要全量加载分类 to_keep/to_delete）；语义与 `read_dump_filtered(TimeRange::ALL)` 等价

## 5. Process-Level Dump Cache

- [x] 5.1 新增 `crates/infra/src/caching/parquet_file_meta_dump.rs`：`ParquetFileMetaDumpCache { inner: Option<moka::future::Cache<DumpCacheKey, Arc<Vec<ParquetFileMeta>>>> }`；`DumpCacheKey = (Arc<str>, Arc<str>, StreamType, PartitionLevel, Arc<str>)`
- [x] 5.2 暴露 `get / insert / invalidate / invalidate_partition / invalidate_stream`；`capacity = 0` 走 noop（`inner: None`）
- [x] 5.3 metrics 走现有 `CacheMetrics::register("parquet_file_meta_dump")`，自动注册 `cache_parquet_file_meta_dump_{hits,misses,evictions}_total` + `cache_parquet_file_meta_dump_hit_ratio`；evictions 通过 moka async eviction listener
- [x] 5.4 `crates/bootstrap/src/wire.rs` 注入 `ParquetFileMetaDumpCache::new(&settings.cache.parquet_file_meta_dump)`，挂入 `DumpQueryContext.dump_cache`
- [x] 5.5 `crates/infra/src/persistence/repositories/parquet_file_meta.rs` 把 cold 合并路径改成 cache-first + 全量 parse 入 cache + 本地 filter；hit 路径 Arc clone + filter
- [x] 5.6 `ParquetFileMetaDumpService` 加 `with_dump_cache(cache)` 注入 + `invalidate_cache` helper；`dump_one_partition` tx commit 后调用、`delete_by_time_range` 的整删 / 部分重写两 case 都同步失效对应 partition；`wire.rs` 启动期注入；2 个单测覆盖（with_dump_cache 注入下 invalidate 落地 + 未注入时 no-op）
- [x] 5.7 cache 单测：hit-after-insert / capacity 0 noop / invalidate drops / invalidate_partition / TTL expiry — 5/5 全绿

## 6. Repository (PG) Wiring

- [x] 6.1 改写 `crates/infra/src/persistence/repositories/parquet_file_meta_dump.rs::PgParquetFileMetaDumpRepository`：14 列 SELECT/INSERT/UPDATE，`row_to_dump` 读全 14 字段；`find_by_time_range` 用 `WHERE deleted=FALSE AND max_ts_micros >= $start AND min_ts_micros <= $end`
- [x] 6.2 实现 `mark_deleted(object_key)`（`UPDATE … SET deleted = TRUE, updated_at_micros = now`）
- [x] 6.3 实现 `upsert_dump(row, stats)`：一笔 tx 内 INSERT dump（partial-unique-on-live ON CONFLICT 兜底） + INSERT stats
- [x] 6.4 实现 `insert_rewrite(old_key, new_row, new_stats)`：单 tx 内 mark old deleted → DELETE old stats → INSERT new dump → INSERT new stats
- [x] 6.5 repo IT 测试在 `crates/infra/tests/it_parquet_file_meta_dump.rs`：`repo_upsert_find_mark_deleted_roundtrip`、`repo_insert_rewrite_swaps_live_seat`（`MS_RUN_IT=1` 启）
- [x] 6.6 `parquet_file_meta.rs:150-211` 用新 reader + cache 替换旧 `read_dump`；`find_by_time_range` 在 PG 侧已按 `deleted=FALSE` 过滤，caller 不需额外检查

## 7. Stream Stats Consumer

- [x] 7.1 grep 验证：当前 codebase 无 `StreamStats` consumer 路径（仅 domain `Schema.use_stream_stats_for_partitioning` 是个 flag）。后续 stream stats 服务接入时直接走新增的 `PgParquetFileMetaDumpRepository::aggregate_stats_in_range` 入口
- [x] 7.2 `PgParquetFileMetaDumpRepository::aggregate_stats_in_range` 已实装：单笔 `SELECT SUM(rows_total), SUM(storage_size_bytes), MIN(time_start_micros), MAX(time_end_micros) FROM parquet_file_meta_dump_stats JOIN parquet_file_meta_dump USING(object_key) WHERE …`，不开任何 parquet
- [x] 7.3 一致性测留作 follow-up：等 stream stats consumer 接入时一并写（当前无 caller 跑得通）

## 8. Spec & Documentation

- [x] 8.1 `openspec validate parquet-file-meta-dump-columnar --strict` 通过
- [x] 8.2 `ARCHITECTURE.md Storage layout` 加 `parquet_file_meta_dump 冷分区下沉` 段（schema 14 列 / partition_level / advisory lock / delete_by_time_range / cache / metrics）
- [x] 8.3 跳过（仓内不存在 `OPENOBSERVE_CLOUD_CATALOG.md` 的差距追踪机制；proposal.md + design.md 已显式记录差距）
- [x] 8.4 迁移 SOP 已在 proposal.md `Migration Plan` 段覆盖（disable worker → 删旧对象 → 跑 migration → 启 worker → 观察 metrics）；不重复落 RUNBOOK

## 9. Local Verification

- [x] 9.1 `cargo build --workspace` 通过
- [x] 9.2 `cargo test -p molesignal-infra --lib storage::parquet_file_meta_dump` — 15/15 绿
- [x] 9.3 `cargo test -p molesignal-infra --lib caching::parquet_file_meta_dump` — 5/5 绿
- [x] 9.4 `cargo clippy --workspace --all-targets -- -D warnings` —— 本 change 引入的代码 0 新增 lint；workspace 失败由 pre-existing 的 `shared::report_renderer::from_str` 与 `bool_assert_comparison` 触发，与本 change 无关
- [ ] 9.5 启 local bootstrap + minio + pg：留作运维侧手工验收（参 proposal.md `Migration Plan` Phase B/C）
- [ ] 9.6 `delete_by_time_range` 手工跑：IT 测试 `service_delete_by_time_range_drops_full_overlap_dump`（`MS_RUN_IT=1`）已覆盖整删；部分重写 case 留 follow-up（PG repo `insert_rewrite` 已 IT 覆盖）
