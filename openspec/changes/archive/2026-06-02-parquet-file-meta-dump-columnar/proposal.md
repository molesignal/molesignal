## Why

冷分区 `ParquetFileMeta` 已经在 `PgParquetFileMetaRepository::find_by_time_range` 走 dump 兜底，但 dump parquet 用单列 `meta_json: Utf8`，每次冷查都得 JSON deserialize 整 partition，不能 Arrow 谓词下推；同时缺 dump 聚合 stats 表、缺并发控制、partition 只能整覆盖。冷查/合规删除/stats 刷新三条路径每年都在为这个数据形态付重复的解析与 IO 成本，本次重写一次性消掉。

## What Changes

- **BREAKING**: dump parquet schema 从单列 JSON 换成 columnar 多列（`id, org_id, stream, stream_type, date, object_key, deleted, rows, size_bytes, time_start_micros, time_end_micros, min_values_jsonb, max_values_jsonb, updated_at_micros`）；旧 JSON dump 文件不再读，迁移期一次性归档/删除。
- **BREAKING**: `parquet_file_meta_dump` 表加列（`partition_key, partition_level, deleted, min_ts_micros, max_ts_micros, rows_in_dump, size_bytes`），主键改为 `(org_id, stream, stream_type, partition_level, partition_key)`；老表 drop 重建。
- 新增 `parquet_file_meta_dump_stats` 表：per dump file 聚合 `(rows_total, files_total, time_start_micros, time_end_micros, storage_size_bytes, updated_at_micros)`。
- 配置加 `[storage.parquet_file_meta_dump] partition_level: "daily" | "hourly"`，默认 `daily` 保现状；同时加 `[cache.parquet_file_meta_dump] capacity / ttl_secs`。
- `ParquetFileMetaDumpService::dump_one_partition` 用 PG advisory transactional lock 防并发，拿不到 → 计为 `partitions_skipped{reason="locked"}`。
- 新增 `ParquetFileMetaDumpService::delete_by_time_range`：保留行重写为新 dump → 老 dump mark `deleted=true` + stats 同步删/写新；retention/合规删除走这条路径。
- 读端：`parse_dump_bytes` 改按列直构 `Vec<ParquetFileMeta>`；Parquet reader 上推 `time_end_micros >= range.start AND time_start_micros <= range.end` 谓词；新增进程内 LRU `(org, stream, stream_type, partition_key) → Arc<Vec<ParquetFileMeta>>`。
- stream stats 聚合改读 `parquet_file_meta_dump_stats`，不再打开 dump 文件。

## Capabilities

### New Capabilities
（无）

### Modified Capabilities
- `storage`: ParquetFileMeta Dump Spillover 整体重写（schema / partition / 并发 / 删除路径 / stats writeback）。
- `caching`: 新增 `parquet_file_meta_dump` 进程内缓存 requirement + invalidation 语义。

## Impact

- **代码**：`crates/infra/src/storage/parquet_file_meta_dump_{writer,reader,service}.rs`、`crates/infra/src/persistence/repositories/{parquet_file_meta_dump,parquet_file_meta}.rs`、`crates/domain/src/storage/mod.rs`、`crates/config/src/settings.rs`、`crates/bootstrap/src/wire.rs`（注入新 cache、配 partition_level）。
- **DB**：新 migration `<date>_parquet_file_meta_dump_columnar.sql`，drop+recreate `parquet_file_meta_dump`，新建 `parquet_file_meta_dump_stats`。
- **对象存储**：dump key 仍为 `{org}/_parquet_file_meta_dump/{stream_type}/{stream}/{partition_key}.parquet`（hourly 时 `partition_key = YYYY-MM-DD-HH`）；老 daily JSON dump 文件由 retention sweep 在迁移窗口内删除。
- **指标**：新增 `cache_parquet_file_meta_dump_{hits,misses,evictions}_total`、`cache_parquet_file_meta_dump_hit_ratio`、`parquet_file_meta_dump_partitions_skipped_total{reason="locked"}`、`parquet_file_meta_dump_delete_partitions_rewritten_total`。
- **配置**：`[storage.parquet_file_meta_dump].partition_level`、`[cache.parquet_file_meta_dump].{capacity, ttl_secs}`；老 `cold_after_days / interval_secs / max_partitions_per_tick` 保留。
- **不影响**：tantivy 索引格式（独立 change `tantivy-puffin-migration` 处理）、ingest/query 热路径热数据查询行为。
