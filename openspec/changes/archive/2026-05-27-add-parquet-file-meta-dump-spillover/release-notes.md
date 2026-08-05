# Release Notes Draft — `add-parquet-file-meta-dump-spillover`

> 合并到 `CHANGELOG.md` 或下一次 release notes 时使用以下条目。

## Behavior change

- **ParquetFileMeta 冷分层默认开启**：每小时一轮 worker 扫主 `parquet_file_meta` 表，找
  `time_end_micros < today - 30 days` 的 `(org, stream, stream_type, date)`
  分区，把 live ParquetFileMeta 行序列化成 parquet 上传到
  `{org}/_parquet_file_meta_dump/{stream_type}/{stream}/{date}.parquet`，在同一 PG 事务
  里 INSERT `parquet_file_meta_dump` 索引行 + DELETE 主表行。
- **查询路径透明跨冷热合并**：`ParquetFileMetaRepository::find(time_range)` 在
  `time_range.start < today - cold_after_days` 时自动加载相关 dump.parquet，与
  主表结果合并、按 `ParquetFileMeta.id` 去重、按 `time_range.start` 排序。
- **首次启动注意**：老部署升级后 worker 会陆续把历史冷分区搬到 object_store，
  受 `max_partitions_per_tick`（默认 100）速率约束；进度可经
  `parquet_file_meta_dump_partitions_written_total` 计数器观察。
- **新增 5 个 Prometheus 指标**：
  - `parquet_file_meta_dump_partitions_written_total` Counter
  - `parquet_file_meta_dump_rows_written_total` Counter
  - `parquet_file_meta_dump_partitions_skipped_total{reason=empty|error|locked|duplicate_id}` Counter
  - `parquet_file_meta_dump_query_hits_total` Counter
  - `parquet_file_meta_dump_query_load_seconds` Histogram

## How to opt out

`conf/config.toml`：

```toml
[storage.parquet_file_meta_dump]
enabled = false
```

关闭后 worker 不再 tick，查询路径退化到只读主表；**已 dump 的对象与索引行保留
不动**——再次 `enabled=true` 启动时无需手工迁移即可恢复。

## Tuning

| 字段 | 默认 | 含义 |
|---|---|---|
| `cold_after_days` | `30` | 分区 date 比 today 早多少天才算冷 |
| `interval_secs` | `3600` | worker tick 周期 |
| `max_partitions_per_tick` | `100` | 单 tick 最多 dump 多少分区 |

## Rollback

回滚到上一版本：worker 不会跑、主表也不会被新代码 DELETE，但已写入的 dump 对象
+ `parquet_file_meta_dump` 表行保留。**注意**：旧版本不知道这些 dump，主表 query 将只
看到热窗口范围内的 ParquetFileMeta；要恢复对冷分区的可见性，必须升级回新版本（或
follow-up 工具回灌）。

## Schema migration

`20260701000005_parquet_file_meta_dump.sql` 新建 `parquet_file_meta_dump` 表，含
`(org_id, stream, stream_type, date)` unique 索引与 `created_at_micros` 索引；
迁移可逆（DROP 即可），不影响已有数据。
