## Context

`parquet_file_meta_dump` 自 `2026-05-27-add-parquet-file-meta-dump-spillover` 上线后，写端（compactor 周期把 cold partition 落 parquet + 一笔 `parquet_file_meta_dump` 索引行 + 删 `parquet_file_meta`）与读端（`PgParquetFileMetaRepository::find_by_time_range:186` 触发 `read_dump`）都已经接通。但读写两侧的数据形态都偏简陋：

- **dump parquet**：单列 `meta_json: Utf8`，每行一条 JSON-encoded `ParquetFileMeta`（`parquet_file_meta_dump_writer.rs:20-26`）。当初这样选是为了 schema 演化兜底，但代价是任何冷查都得拉全文件 + JSON parse 全 partition。
- **dump 索引表**：`(org, stream, stream_type, date, object_key, rows_in_dump, created_at)` 没有时间范围列、没有 `deleted`，所以 query 端无法在 PG 侧按时间裁剪 dump，只能全部 GET。
- **没有 dump 聚合 stats**：stream stats 刷新只能打开 dump 文件累加。
- **没有并发控制**：多 compactor 实例可能扫到同 partition，靠 `tx { INSERT dump + DELETE main }` 兜底 race，浪费 IO/PG 连接。
- **partition 粒度固定 daily**：一个 1 小时冷查也要拉一整天 dump。

`storage` 与 `caching` 两个 capability 的当前 requirement 都写死了 "single-column JSON parquet" 与 "30-day cold_after_days" 语义，需要同步演进。

## Goals / Non-Goals

**Goals:**
- 把 dump 数据从 "1 列 JSON" 升级到 columnar 多列 Parquet，让 query 端能在 Arrow 层做谓词下推、按列读所需字段，**而不是先 JSON parse 再 retain**。
- 让 `parquet_file_meta_dump` 表自己就能裁剪：加 `min_ts_micros / max_ts_micros`，PG 侧 `WHERE` 拦掉无关 dump。
- 新增 `parquet_file_meta_dump_stats` 表，stream stats 路径不再打开 dump 文件。
- 支持 `partition_level: Daily | Hourly`（默认 Daily 保现状）；hourly 模式给 cold 高频小窗查询提速。
- `delete_by_time_range` 走"保留行重写新 dump + 老 dump mark deleted"路径，给 retention/合规删除一条精确粒度的清理通道。
- PG advisory lock 把并发 dump 同 partition 这件事按下，省 IO 与连接。
- 进程内 `Arc<Vec<ParquetFileMeta>>` cache 解决"短时间窗内反复跨 cold 边界查"的重复 GET+parse 成本。

**Non-Goals:**
- **不做向后兼容**：旧 JSON dump 文件不再被读取；迁移期一次性归档/删除（用户已确认 clean break）。
- 不引入 DataFusion in-memory table 处理 dump；dump 文件 row 数（一天 ~ 几千行）小，Rust filter + Arrow pushdown 就够，留给未来。
- 不做 dump-of-dump merge（openobserve `merge.rs` 那套）；目前 daily/hourly partition 量级用不上，留作 follow-up。
- 不动 ingest/query 热路径在 hot tier（`parquet_file_meta` 主表）的任何行为。
- 不动 tantivy 索引格式或 sidecar key；这部分进独立 change `tantivy-puffin-migration`。

## Decisions

### D1 — Dump parquet schema: columnar 多列，不抄 openobserve 全集

molesignal `ParquetFileMeta`（`crates/domain/src/storage/mod.rs`）字段是 `{id, org_id, stream, stream_type, object_key, time_range, rows, size_bytes, min_values, max_values, deleted}`，没有 openobserve 那一份的 `records / compressed_size / index_size / flattened / account / segment_ids / row_group_size`。

dump parquet schema 取 molesignal `ParquetFileMeta` 现有字段的最小投影：

| 列 | Arrow 类型 | 来源 |
|---|---|---|
| `id` | `Utf8` | `ParquetFileMeta.id.0` |
| `org_id` | `Utf8` | `ParquetFileMeta.org_id.0` |
| `stream` | `Utf8` | `ParquetFileMeta.stream` |
| `stream_type` | `Utf8` | `"logs" / "metrics" / "traces" / "extend"` |
| `date` | `Utf8` | 写时 partition 的 `YYYY-MM-DD`（hourly 模式时也只写日期，hour 在 `partition_key`） |
| `object_key` | `Utf8` | `ParquetFileMeta.object_key` |
| `deleted` | `Boolean` | `ParquetFileMeta.deleted` |
| `rows` | `Int64` | `ParquetFileMeta.rows` (cast u64→i64) |
| `size_bytes` | `Int64` | `ParquetFileMeta.size_bytes` (cast u64→i64) |
| `time_start_micros` | `Int64` | `ParquetFileMeta.time_range.start.0` |
| `time_end_micros` | `Int64` | `ParquetFileMeta.time_range.end.0` |
| `min_values_json` | `Utf8` | `serde_json::to_string(&ParquetFileMeta.min_values)` |
| `max_values_json` | `Utf8` | `serde_json::to_string(&ParquetFileMeta.max_values)` |
| `updated_at_micros` | `Int64` | dump 写入时刻 |

**为什么 `min/max_values` 仍走 JSON**：molesignal `min_values` 是 `serde_json::Map<String, Value>`，按动态 stream schema 决定列；把它变成结构化列要求 dump schema 跟 stream schema 同步，破坏跨 stream 复用单个 reader 的能力。`min_values_json` 用 Utf8 兜底，避免 dump schema 失稳；这也是与 openobserve dump 差异最大的一点。

**Alternatives considered**：
- (A) 抄 openobserve 全 15 列 schema —— 多出来的 5 列（`records, compressed_size, index_size, flattened, segment_ids`）在 molesignal 没有语义，硬塞会引入死字段。
- (B) `min/max` 走 Arrow `Map<Utf8, …>` —— Arrow 的 Map 列对每 stream schema 都得统一 value 类型，处理 Json/Int/Utf8 混杂的 mins 时类型选择会反复折磨，不如 JSON 兜底。

### D2 — Partition key 与 `partition_level` 配置

- `partition_level: Daily`（默认）→ `partition_key = "YYYY-MM-DD"`，object key `…/_parquet_file_meta_dump/{stream_type}/{stream}/2026-01-15.parquet`，行为同今。
- `partition_level: Hourly` → `partition_key = "YYYY-MM-DD-HH"`，object key `…/2026-01-15-13.parquet`。
- 表里 `partition_level` 列存 `"daily" | "hourly"`，主键改为 `(org_id, stream, stream_type, partition_level, partition_key)`。query 端按 partition_level + partition_key 找 dump。
- 同一 stream 不允许混合粒度（先 daily 后 hourly），如果切换 partition_level，新 partition 用新粒度，已有 daily dump 不重写（除非显式触发 `delete_by_time_range` 重新 dump）。

**Why**：保住 daily 现有用户的体验同时给 hourly 让位 high-fanout 冷查。两套粒度共存而不强制迁移是为了不堵塞配置变更。

### D3 — `parquet_file_meta_dump_stats` 表语义

```
CREATE TABLE parquet_file_meta_dump_stats (
  object_key          TEXT PRIMARY KEY REFERENCES parquet_file_meta_dump(object_key) ON DELETE CASCADE,
  rows_total          BIGINT NOT NULL,
  files_total         BIGINT NOT NULL,
  time_start_micros   BIGINT NOT NULL,
  time_end_micros     BIGINT NOT NULL,
  storage_size_bytes  BIGINT NOT NULL,
  updated_at_micros   BIGINT NOT NULL
);
```

写时点：`dump_one_partition` 在 PUT 成功 + 拿到 lock 后，把聚合一并写入这张表（与 `INSERT parquet_file_meta_dump` 同一 tx）。`delete_by_time_range` 重写时同步更新。stream stats 服务（未来或现有）按 `object_key in (...)` 一次 PG 查询拿到聚合，不再触 object store。

**Why 一张独立表而不是 dump 表上多几列**：聚合列的语义跟 `parquet_file_meta_dump` 索引行职责不同（dump 行 = "存在性 + 路由"，stats 行 = "汇总"），独立表 + FK ON DELETE CASCADE 让重写/删除时的一致性容易看。

### D4 — 并发控制：PG advisory transactional lock

`dump_one_partition` 在事务里先：

```sql
SELECT pg_try_advisory_xact_lock(
  hashtext(format('%s|%s|%s|%s|%s',
    org_id, stream, stream_type, partition_level, partition_key))
);
```

拿不到 → 当前 tick 把这个 partition 计为 `partitions_skipped{reason="locked"}`，跳过；下一 tick 重试。tx 结束（commit 或 rollback）自动释放锁，省掉 keepalive 机制。**不抄 openobserve 的 `file_list_jobs` 表 + consistent hash + keepalive** —— 那套需求来自 keepalive across 长任务（百万行 dump），molesignal 单 partition row 数在千级别，几秒内完成，advisory lock 已足够。

**Alternatives considered**：
- (A) `parquet_file_meta_dump_jobs` 表 + consistent-hash + node ttl —— 增 PG 表与状态机，运维面变大。
- (B) `SELECT … FOR UPDATE SKIP LOCKED` 在 `parquet_file_meta` 上锁 —— 锁住主表会扰动 retention/compactor 路径。

### D5 — `delete_by_time_range` 半重写流程

```
1. tx_begin
2. SELECT object_key, partition_key, partition_level
   FROM parquet_file_meta_dump
   WHERE (org, stream, stream_type) match
     AND (time_start_micros, time_end_micros) overlaps target_range
     AND deleted = false
   FOR UPDATE
3. for each dump:
   a. GET object → parse columnar bytes → Vec<ParquetFileMeta>
   b. partition rows by `target_range`: to_keep / to_delete
   c. if to_keep == 全部 → 无 overlap，skip
   d. if to_keep == 空 → 整 dump 删除：
        - mark parquet_file_meta_dump.deleted = true
        - DELETE parquet_file_meta_dump_stats WHERE object_key = $
        - schedule object delete in cleanup queue (异步)
   e. else（部分保留）:
        - serialize columnar(to_keep) → bytes
        - new_object_key = base/{partition_key}.{rewrite_seq}.parquet
        - PUT new_object_key
        - INSERT parquet_file_meta_dump (new row, deleted=false, new partition_key suffix or rewrite_seq)
        - INSERT parquet_file_meta_dump_stats (new aggregates)
        - mark old parquet_file_meta_dump.deleted = true
        - DELETE parquet_file_meta_dump_stats WHERE object_key = old
        - schedule old object delete (异步)
4. tx_commit
5. async: storage.delete(scheduled keys)
```

**Why "新 object key + 老的 mark deleted"**：保住 cold reader 在 tx commit 前后都能拿到一致集合；老 dump 真删走 object store 异步路径，失败有 retry 兜底。`rewrite_seq` 是 1 起的递增整数，写在 object key 末段（避免覆盖前一份）。

### D6 — Reader 改造：列读 + 谓词下推 + 进程内 cache

```rust
pub async fn read_dump_filtered(
    store: Arc<dyn ObjectStore>,
    object_key: &str,
    time_range: TimeRange,
) -> Result<Vec<ParquetFileMeta>>
```

实现：
1. 进程内 cache 查 `(org, stream, stream_type, partition_level, partition_key) → Arc<Vec<ParquetFileMeta>>`，命中且未过 TTL 直接克隆 Arc + 本地 filter。
2. miss → `ProductionObjectStore::get`（自动走 disk cache）→ `ParquetRecordBatchReaderBuilder`，**用 `ArrowPredicate` 注册 `time_end_micros >= range.start AND time_start_micros <= range.end`**，让 reader 在 row group + page 层提前剪。
3. 解列直构 `ParquetFileMeta`：`Int64`/`Utf8` 列 batch 化 cast；`min_values_json/max_values_json` 按需 lazy parse（构 `ParquetFileMeta` 时立即 parse 也行，看 profile）。
4. 写 cache（Arc 全集，filter 前的），返回 filter 后的 Vec。

**Cache invalidation**：
- `ParquetFileMetaDumpRepository::mark_deleted(object_key)` 时同步 evict cache 对应 entry。
- `ParquetFileMetaDumpRepository::upsert(...)`（写新 dump）时按 `(org, stream, stream_type, partition_level, partition_key)` 前缀失效。
- TTL 兜底（默认 `ttl_secs = 600`）。

**Cache size budget**：每个 entry ~ `rows_per_partition * sizeof(ParquetFileMeta) ~ 几 KB ~ 几十 KB`；`capacity = 10_000` partition 占用 ~ 几百 MB，与现有 parquet_meta cache 同量级。

### D7 — Migration：clean break

```
1. 部署前 op：列出所有现存 _parquet_file_meta_dump 对象（aws s3 ls 或等价），归档/删除（按团队偏好）。
2. migration SQL：
   - DROP TABLE IF EXISTS parquet_file_meta_dump (CASCADE);
   - CREATE TABLE parquet_file_meta_dump (new schema, with partition_level/partition_key/deleted/min_ts_micros/max_ts_micros);
   - CREATE TABLE parquet_file_meta_dump_stats (...);
   - CREATE INDEX idx_parquet_file_meta_dump_query
       ON parquet_file_meta_dump (org_id, stream, stream_type, time_start_micros, time_end_micros)
       WHERE deleted = false;
3. 部署后：worker 自然从 cold partition 重新生成 dump。
```

**Why clean break**：用户已确认；保留双 reader（嗅探单列 JSON vs columnar）会让 reader 一直背 dead code，schema 也得分两套，得不偿失。

## Risks / Trade-offs

- **[Risk] 迁移期 dump 不可读**：DROP 后直到第一个 tick 完成前，冷数据查询拿不到 dump 结果。**Mitigation**：先在维护窗口跑 `delete_by_time_range(full_range)` 重写 → 拒做，因为旧 reader 已废；改为 (a) 部署前 op 阶段把"未来 N 天冷查"的 partition 提前手动 `dump_one_partition`，或 (b) 接受迁移窗口内冷查可能漏 1~2 tick（默认 1 小时）。文档里需要标红。
- **[Risk] `min_values_json` 仍是 JSON**：ParquetFileMeta 构造时要 parse 一次 JSON，cache 命中后省 IO 但不省 JSON。**Mitigation**：cache value 直接持 `Arc<Vec<ParquetFileMeta>>`，ParquetFileMeta 内部 `min_values: Map<String, Value>` 已经是 parsed；JSON decode 只在 cache miss 路径发生一次。
- **[Risk] hourly partition 数量爆炸**：1 stream * 30 cold days * 24 = 720 个 dump 对象/月 → S3 list 成本可观。**Mitigation**：partition_level 默认仍 `daily`；切 hourly 只在 stream-level/org-level 主动开启，并提示运营层先估容量。
- **[Risk] PG advisory lock 与跨 partition 调度交互**：多 worker 同时 tick → 大量 try-lock 失败 → 多数 partition 被计为 `skipped{locked}` 但其实是正常排他。**Mitigation**：`skipped{locked}` 不算 error 指标，alerting 跳过；下 tick 自然消化。
- **[Trade-off] Cache 不分进程间共享**：querier 横向扩多节点时不同节点重复 GET 同一 dump。**Mitigation**：靠 ProductionObjectStore disk cache 在跨节点冷数据上承担最大头；进程内 cache 是次级减负，未来可以接入 shared `redis` 等等。
- **[Trade-off] `delete_by_time_range` 不做 dump-of-dump merge**：长期可能留下许多 `*.{seq}.parquet` 小文件。**Mitigation**：本 change non-goal；compactor follow-up change 接入 dump 合并。

## Migration Plan

Phase A —— 准备（**运维操作**）：
1. 在维护窗口前 1 天，把 `[storage.parquet_file_meta_dump]` 改 `enabled = false` 停 worker，等当前 tick 跑完。
2. 删除（或归档）所有 `*/_parquet_file_meta_dump/**` 对象（命令视部署而定）。
3. 备份 `parquet_file_meta_dump` 表 schema/数据（兜底）。

Phase B —— 部署：
1. 部署新版本，启动会跑 migration：drop 旧表 + create 新表 + 新建 stats 表。
2. 把 `[storage.parquet_file_meta_dump] enabled = true`，worker 自然在下一 tick 重建 cold partition dump。

Phase C —— 验证：
- 观察 `parquet_file_meta_dump_partitions_written_total` 速率，达到稳态后跑跨冷边界查询，比对 `parquet_file_meta_dump_query_hits_total` 与 hot+cold 结果集是否完整。
- 观察 `cache_parquet_file_meta_dump_hit_ratio`：稳态期望 > 0.3（取决于查询模式）。

**Rollback**：回到上一版本 + revert migration（migration 文件需要带 `down.sql`，或仍走"再次 drop + 回旧 schema"的运维流程）。生产数据 dump 重建可在回滚后自然完成。

## Open Questions

- **Q1**：`object_key` 上的 `rewrite_seq` 怎么命名？候选：`{partition_key}.r1.parquet` / `{partition_key}.parquet` + 单独表列存 `revision`。倾向命名方案 A（无需多列；reader 不关心 seq），但要确认 S3 list 端不依赖固定后缀。
- **Q2**：cache 的 LRU 实现要不要复用 `caching::parquet_file_meta`（如果它的 key 与本 cache key 兼容），还是新建一个独立 `caching::parquet_file_meta_dump`？取决于现有 `parquet_file_meta` cache 的内部 trait 是否已经能容纳 partition-level 的复合 key。
- **Q3**：`delete_by_time_range` 的 async object delete 路径要不要复用 compactor 的 deletion sweep？compactor 已经有"标 deleted → 异步 GC"的语义，能复用最好。
