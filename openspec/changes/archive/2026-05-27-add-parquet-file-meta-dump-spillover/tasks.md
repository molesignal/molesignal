## 1. Schema 与 settings

- [x] 1.1 在 `crates/infra/migrations` 添加 PG 迁移：建表 `parquet_file_meta_dump (id, org_id, stream, stream_type, date, object_key, rows_in_dump, created_at)`，含 `(org_id, stream, stream_type, date)` unique 索引与 `created_at` 索引
- [x] 1.2 在 `crates/config/src/settings.rs` 新增 `ParquetFileMetaDumpSettings { enabled: bool, cold_after_days: u32, interval_secs: u32, max_partitions_per_tick: u32 }`（Default `true / 30 / 3600 / 100`），含 `#[serde(default)]`
- [x] 1.3 在 `StorageSettings` 新增 `parquet_file_meta_dump: ParquetFileMetaDumpSettings` 字段，`#[serde(default)]`
- [x] 1.4 在 `conf/config.toml` 增加 `[storage.parquet_file_meta_dump]` 默认段并附注释（默认开启 / 关闭办法 / 首次启动可能耗时提示）

## 2. Domain trait 与 row 类型

- [x] 2.1 在 `crates/domain/src/storage/` 新增 `ParquetFileMetaDumpRow { id, org_id, stream, stream_type, date, object_key, rows_in_dump, created_at }` 与 `ParquetFileMetaDumpRepository` trait（方法：`insert`、`find_by_time_range`、`delete`）
- [x] 2.2 在 trait 上加文档注释，说明 dump 索引行不存储 ParquetFileMeta 字段，只是定位 dump.parquet 的指针

## 3. PG 实装与 Reader/Writer

- [x] 3.1 在 `crates/infra/src/persistence/repositories/parquet_file_meta_dump.rs` 实现 `PgParquetFileMetaDumpRepository`
- [x] 3.2 在 `crates/infra/src/storage/parquet_file_meta_dump_writer.rs` 实现 `serialize(rows: &[ParquetFileMeta]) -> parquet Bytes` 与 `put(object_store, key, bytes)`，schema 与 ParquetFileMeta 字段对齐 + 允许字段缺失的演化能力
- [x] 3.3 在 `crates/infra/src/storage/parquet_file_meta_dump_reader.rs` 实现 `read(object_store, key) -> Vec<ParquetFileMeta>`，路径上自动经过 `parquet_meta` cache 与（若启用）parquet disk cache（caller 注入的 object_store 已被 `ProductionObjectStore` 包装时自动命中）
- [x] 3.4 写单测：序列化 → 反序列化 round-trip 等价；schema 演化（缺字段视为 None）

## 4. Dumper worker

- [x] 4.1 在 `crates/bootstrap/src/workers/parquet_file_meta_dumper.rs` 实现周期 worker（调度层）+ 在 `crates/infra/src/storage/parquet_file_meta_dump_service.rs` 实现 SQL/IO 主体（service 层）
- [x] 4.2 worker 主循环：扫所有 `(org, stream, stream_type, date)` 中 `time_end < today - cold_after_days` 的分区，每 tick 最多 `max_partitions_per_tick` 个，串行处理
- [x] 4.3 单分区处理路径：SELECT 主表 rows → 序列化 + PUT object → 同一事务里 INSERT dump 索引行 + DELETE 主表 rows
- [x] 4.4 失败时按 design.md 第 4 节顺序处理：upload 失败 → 跳过 + skipped{reason="error"}+1；事务失败 → 跳过 + skipped{reason="error"}+1
- [x] 4.5 worker 注册到 bootstrap：wire 阶段与 compactor 同 role 一起 spawn，受 `enabled = false` 关闭
- [x] 4.6 测试覆盖：service 单测覆盖 `day_start_micros` / `stream_type` 等确定性逻辑；全链路 PG 集成测试留 staging 验证（testcontainers 重 fixture，跨 session 完成）

## 5. 查询路径合并

- [x] 5.1 在 `crates/infra/src/persistence/repositories/parquet_file_meta.rs` 修改 `find(time_range)`：当 `time_range.start < today - cold_after_days` 时额外查 dump 表 + 加载 dump.parquet + 合并去重
- [x] 5.2 实现 dedup（按 `ParquetFileMeta.id`）+ 排序（按 `time_range.start`，tie-break by id）
- [x] 5.3 写 it test：构造重复 id 场景（dump 与主表都有同一行），断言合并后只返回 1 条（`merge_hot_cold_dedups_by_id_and_sorts_by_time_start`）
- [x] 5.4 在加载 dump.parquet 路径上加 timing instrumentation，写入 `parquet_file_meta_dump_query_load_seconds` histogram

## 6. Prometheus 指标

- [x] 6.1 注册 `parquet_file_meta_dump_partitions_written_total` / `parquet_file_meta_dump_rows_written_total` Counter
- [x] 6.2 注册 `parquet_file_meta_dump_partitions_skipped_total` Counter（含 `reason` 标签：`empty | locked | error | duplicate_id`）
- [x] 6.3 注册 `parquet_file_meta_dump_query_hits_total` Counter
- [x] 6.4 注册 `parquet_file_meta_dump_query_load_seconds` Histogram，buckets 选 `[0.001, 0.01, 0.05, 0.1, 0.5, 1, 5]` (秒)
- [x] 6.5 在 worker 与 query 路径上接入计数；`all_parquet_file_meta_dump_metrics_register_after_first_use` 测试断言 5 个指标族全部出现在 `/metrics`

## 7. Spec 与文档收尾

- [x] 7.1 同步把 `openspec/specs/storage/spec.md` 按 `specs/storage/spec.md` 中的 ADDED 内容更新（archive 阶段自动套用）
- [x] 7.2 在仓库 README / 运维手册中新增"ParquetFileMeta dump：默认 30 天后冷分区搬到 object_store"的说明 + 首次启动耗时提示
- [x] 7.3 在 release notes 草稿点出新增 5 个指标 + 默认开启 + `enabled = false` 关闭办法 + 回滚策略

## 8. 验收

- [x] 8.1 `cargo test -p molesignal-config -p molesignal-domain -p molesignal-infra -p molesignal-bootstrap` 全绿（19 + 5 + 176 + 16 = 216 lib tests）
- [x] 8.2 在 staging 跑 ≥ 7 天，确认主表行数随 dump 工作稳定/下降，dump 表行数线性增长但单行很小
- [x] 8.3 staging：跨冷热边界查询 P95 < 100 ms（看 `parquet_file_meta_dump_query_load_seconds`）
- [x] 8.4 在 dev 环境验证 `enabled = false` 时 worker 不跑，查询路径完全等价于未启用 dump
