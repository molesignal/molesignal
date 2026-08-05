## Why

`ParquetFileMeta` 是查询路径的总目录：`ParquetFileMetaRepository::find(time_range)` 给 planner 返回所有候选 parquet 的 `(object_key, time_range, min/max stats)`，是元数据级裁剪的基础。长时间运行（几个月到几年）后，主元数据表（Postgres `parquet_file_meta`）行数会线性膨胀到数千万到上亿级——即便每行只有几百字节，索引扫描和 query plan 拉数也会变慢。Compactor 的合并能减少行数，但只对小文件起作用，老分区的大 parquet 仍然永久占行。

对标 OpenObserve `file_list_dump`（`src/service/file_list_dump.rs:331-389`、`src/infra/.../file_list_dump_*`）：把"冷"分区（按 date 超过阈值）的 file_list 行整体序列化为 parquet 写到 object store，主表删除对应 row；查询路径同时从主表 + dump 表读，合并后去重排序。代价：冷分区查询多一次小 dump 文件加载；收益：主表始终保持小（GB → MB 量级），所有热路径查询的 file_list 检索都很快。

molesignal 当前没有等价机制；当系统跑到生产规模时，元数据查询会成为性能瓶颈。

## What Changes

- 新增"ParquetFileMeta dump"子能力：把超过 `storage.parquet_file_meta_dump.cold_after_days`（默认 30 天）的 `(org, stream, stream_type, date)` 分区的 ParquetFileMeta 行序列化为 parquet 文件，写到 object_store 路径 `{org}/_parquet_file_meta_dump/{stream_type}/{stream}/{date}.parquet`；主 `parquet_file_meta` 表对应 row 删除。
- 新增 `ParquetFileMetaDumpRepository` trait + `PgParquetFileMetaDumpRepository` 实装：跟踪 dump 索引行（`org / stream / stream_type / date / object_key / rows_in_dump / created_at`），不存原始 ParquetFileMeta 字段。
- 新增 dumper worker：与 compactor 同一 bootstrap role，按 `storage.parquet_file_meta_dump.interval_secs`（默认 3600，1 小时）周期扫描可 dump 分区，串行 dump，每轮 dump 完写一条 dump 索引行并删除主表对应行。
- 查询路径修改：`ParquetFileMetaRepository::find(time_range)` 在主表查询返回后，对落在 `cold_after_days` 之外的时间窗额外调用 `ParquetFileMetaDumpRepository::find(time_range)`，加载相关 dump parquet，反序列化为 `Vec<ParquetFileMeta>`，与主表结果合并去重 + 按 `time_range.start` 排序。
- 配置：新增 `[storage.parquet_file_meta_dump]` 段（`enabled`、`cold_after_days`、`interval_secs`），全部带默认值。
- 指标：`parquet_file_meta_dump_partitions_written_total`、`parquet_file_meta_dump_rows_written_total`、`parquet_file_meta_dump_partitions_skipped_total{reason}`、`parquet_file_meta_dump_query_hits_total`、`parquet_file_meta_dump_query_load_seconds`（histogram）。
- 失效与原子性：dump 写入 → 主表 delete 必须保证 dump 文件 + dump 索引行先持久化，再删主表行（先写后删，保证查询时至少能从一处看到）。

## Capabilities

### New Capabilities

(无)

### Modified Capabilities

- `storage`: 在 `ParquetFileMeta Partition Pruning` / `Compactor` 周围新增 "ParquetFileMeta Dump Spillover" 与 "ParquetFileMeta Dump Query Path" 两条 Requirement，覆盖 dump 写入、查询合并、失效原子性、指标。

## Impact

- **配置**：`conf/config.toml` 新增 `[storage.parquet_file_meta_dump]` 段。
- **代码**：
  - `crates/domain`：新增 `ParquetFileMetaDumpRepository` trait + `ParquetFileMetaDumpRow` 类型。
  - `crates/infra/src/persistence/repositories/parquet_file_meta_dump.rs`：PG 实装。
  - `crates/infra/src/storage/parquet_file_meta_dump_writer.rs`：序列化 ParquetFileMeta 列表为 parquet + 上传。
  - `crates/infra/src/storage/parquet_file_meta_dump_reader.rs`：读 dump parquet → `Vec<ParquetFileMeta>`。
  - `crates/bootstrap/src/workers/parquet_file_meta_dumper.rs`：周期 worker。
  - `crates/infra/src/persistence/repositories/parquet_file_meta.rs`：`find` 路径加合并 dump 来源。
- **数据库**：新增 `parquet_file_meta_dump` 表迁移（id / org / stream / stream_type / date / object_key / rows_in_dump / created_at）。
- **对象存储**：新增 `{org}/_parquet_file_meta_dump/...` 前缀；现有 `compactor` retention 不影响该路径（dump 文件由 dumper worker 自己管理生命周期，跟随主表 retention 间接淘汰）。
- **可观测性**：`/metrics` 新增 5 个指标。
- **兼容性**：滚动升级零冲击；默认 `enabled = true`，老部署升级后 worker 自动开始把超过 30 天的分区搬到 dump（首轮可能耗时较长，需要 release notes 提示）。
- **回滚**：禁用 worker（`enabled = false`）即停止 dump；已 dump 数据在 dump 表 + object_store 里，查询路径仍能读，不会丢；如需"全量回灌"主表，提供 follow-up 工具（不在本 change 范围）。
