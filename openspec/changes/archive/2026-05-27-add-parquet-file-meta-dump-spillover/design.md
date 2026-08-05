## Context

`ParquetFileMeta` 主表是查询路径上每次 plan 都要过的中枢索引。当前 Postgres `parquet_file_meta` 单租户实测在 30 天 retention + 中等流量下行数稳定在 10⁵-10⁶ 量级，索引扫描毫秒级。但：

- retention 拉长到 90/180/365 天后行数线性增长。
- Compactor 只对小文件做 merge，老分区的大 parquet 仍永久占行。
- 多租户场景下不同 org 的行混在同一索引里，索引尺寸 = 全部 org 之和。

OpenObserve 的对策（`file_list_dump`）：

```text
Hot tier:     PG file_list   ──>  Q < N days, 全在线索引
Cold tier:    Object-store    ──>  partition-level dump.parquet
              + file_list_dump 索引 (org/stream/date 三列)
```

查询路径：

```text
find(time_range):
  hot = PG file_list query
  if time_range overlaps cold-tier window:
    dumps = file_list_dump.query(time_range)
    cold_metas = for each dump: download dump.parquet → deserialize Vec<ParquetFileMeta>
    merge_sort_dedup(hot + cold_metas)
  else:
    hot
```

`file_list_dump` 索引表只存 `(org, stream, date, dump_object_key, rows_in_dump)` 这种小尺寸 row，不存任何原始 ParquetFileMeta 字段；真正的 ParquetFileMeta 全在 dump.parquet 里。这让 PG 主表始终保持小。

molesignal 当前没有等价机制：所有 ParquetFileMeta 永远在 PG，行数随时间线性增长。

## Goals / Non-Goals

**Goals:**

- 给系统一个"冷热分层"的 ParquetFileMeta 索引，主表保持小（数据库索引扫描始终毫秒级）。
- 查询路径透明：planner 仍然只调一个 `ParquetFileMetaRepository::find(time_range)`，dump 合并由实现自己负责。
- 写入与删除原子性正确：dump 内容 + dump 索引行写完后再删主表行；任何阶段失败都保留主表行（次轮重试）。
- Worker 串行 dump，避免并发争 IO / object_store 限流；周期、阈值、开关全部可配。
- 五个指标覆盖：dump 速率、跳过原因、查询路径加载冷分区的命中数和耗时。

**Non-Goals:**

- 不实现 dump.parquet 的 in-place compact（旧 dump 不会被合并；如果有需要，作为 follow-up）。
- 不实现"主表 → dump → 主表"双向迁移（仅单向 hot → cold）。
- 不动 `compactor` 的合并 / retention 逻辑（dump 仅搬运 ParquetFileMeta 索引行，不动数据 parquet）。
- 不实现 dump 文件的独立 retention（dump 在主表 retention 删除底层 parquet 时也要清理，由配套 logic 处理，跟当前 retention worker 协同）。

## Decisions

### 1. Partition 单位 = `(org, stream, stream_type, date)`

dump 以分区为粒度，与 parquet 写入分区一致（`{org}/{stream_type}/{stream}/{YYYY-MM-DD}/`）。一个分区里通常有几十到几百个 ParquetFileMeta（compactor 已合并过），打包成一个 dump.parquet 大小可控。

**替代方案**：按 `(org, stream, hour)` 更细粒度。否决：dump 文件太多，索引行密度反而高。

**替代方案**：按 `(org, stream, month)` 更粗。否决：跨天的 retention 不好处理（删一天必须重写整个月的 dump）；day 粒度跟 retention/compactor 已有概念对齐。

### 2. dump 文件位置

`{org}/_parquet_file_meta_dump/{stream_type}/{stream}/{date}.parquet`

`_parquet_file_meta_dump` 前缀以 `_` 开头，对齐 `_health` 风格，避免与正常 stream 名冲突。每个分区一个 dump 文件，文件名等于 date，让 reader 能直接按 date 范围列出来。

### 3. 何时算"冷"

`storage.parquet_file_meta_dump.cold_after_days`（默认 30）：分区的 date 早于 `today() - cold_after_days` 时才考虑 dump。

**为什么不是 `time_range.end`**：date 是分区的固定属性，今天写入的数据放在今天的 date 分区即使 `time_range.end` 已经过期；用 date 简化判断。

### 4. dump 写入 / 删除主表的顺序

```text
1. SELECT * FROM parquet_file_meta WHERE (org, stream, stream_type, date) = X AND deleted = false
2. 序列化为 parquet 字节，PUT 到 {org}/_parquet_file_meta_dump/...
3. INSERT INTO parquet_file_meta_dump (org, stream, stream_type, date, object_key, rows_in_dump, created_at)
4. DELETE FROM parquet_file_meta WHERE id IN (...被搬运的 ids...)
```

步骤 3 + 4 在同一 PG 事务里。步骤 1-2 在事务前面（失败 → object 是孤儿，由后续 retention 清理；不重复 PUT，因为 step 4 还没执行）。

**替代方案**：先 DELETE 再 PUT。否决：DELETE 后 PUT 失败 → 主表丢数据；不可接受。

**替代方案**：用 `SELECT FOR UPDATE` 锁主表行避免并发被 compactor 改。是否需要取决于 compactor 是否会动 30 天前的行；如果不会，可以不锁。第一版假设 dumper 只动 cold 分区，compactor 只动 hot 分区，无冲突；后续如果出现 race，再加锁。

### 5. 查询路径合并

`ParquetFileMetaRepository::find(time_range)` 修改为：

```rust
async fn find(&self, time_range: TimeRange) -> Result<Vec<ParquetFileMeta>> {
    let hot = self.find_main_table(time_range).await?;
    let cold_window_start = today_minus(self.cold_after_days);
    if time_range.start < cold_window_start {
        let dump_indices = self.dump_repo.find(time_range).await?;
        let mut cold = Vec::new();
        for dump in dump_indices {
            let metas = self.dump_reader.read(&dump.object_key).await?;
            cold.extend(metas.into_iter().filter(|m| m.time_range.overlaps(&time_range)));
        }
        let mut merged = hot;
        merged.extend(cold);
        merged.sort_by_key(|m| m.time_range.start);
        merged.dedup_by_key(|m| m.id);
        Ok(merged)
    } else {
        Ok(hot)
    }
}
```

`dump_reader.read` 内部命中 `parquet_meta` 缓存（已在 caching capability 实现）+ parquet_disk_cache（独立 change 完成后命中）。冷分区 dump 文件本身较小（几十 KB - 几 MB），加载成本远小于扫所有原始 ParquetFileMeta 行。

**替代方案**：dump 索引表存全部 ParquetFileMeta 字段（dump.parquet 就退化为冗余）。否决：dump 索引行的尺寸又把主表问题复制到 dump 索引表，没解决根本。

### 6. Retention 协同

主表 ParquetFileMeta 删除（`mark_deleted`）由 retention sweep 触发；dump 不自动跟着删（不同 lifecycle）。需要在 retention 时增加：

- 如果某 dump 分区里**所有** ParquetFileMeta 都已被 retention 删（含主表里已不存在的），dumper 下一轮把整 dump 文件 + dump 索引行删除。

实现路径：dumper worker 周期里另外做一遍 "dump GC" 扫，对每个 dump 索引行读 dump.parquet → 检查每个 ParquetFileMeta 对应的 parquet object 是否还在 → 全部不在则删 dump。第一版先不做（dump 索引膨胀缓慢，且 dump.parquet 占盘小），作为 follow-up。**本 change 范围：仅写 + 读路径，不实现 dump GC**。

### 7. 配置 schema

```toml
[storage.parquet_file_meta_dump]
enabled = true
cold_after_days = 30
interval_secs = 3600
# 单 tick 最多 dump 多少分区，防止首次启动时大量积压一次性压垮 IO
max_partitions_per_tick = 100
```

### 8. 指标

| 指标 | 类型 | 标签 |
|---|---|---|
| `parquet_file_meta_dump_partitions_written_total` | Counter | - |
| `parquet_file_meta_dump_rows_written_total` | Counter | - |
| `parquet_file_meta_dump_partitions_skipped_total` | Counter | `reason="empty\|locked\|error"` |
| `parquet_file_meta_dump_query_hits_total` | Counter | - （查询路径触发 dump 加载的次数）|
| `parquet_file_meta_dump_query_load_seconds` | Histogram | - （单次 dump.parquet 加载耗时）|

## Risks / Trade-offs

- **首次启动时积压**：老部署升级后 worker 把所有 ≥ 30 天的分区一次性 dump，可能短时间内压力大。
  - Mitigation：`max_partitions_per_tick` 控制速率；release notes 提示首次开启可能耗时。
- **dump.parquet schema 演化**：ParquetFileMeta 字段未来如果加新列，老 dump 文件读出来缺新字段。
  - Mitigation：dump.parquet 用 `parquet` schema 演化能力（旧文件缺列时读出 None）；反序列化代码必须容忍缺字段。
- **dump 写入 → 主表 DELETE 之间崩溃**：PUT object 成功，dump index INSERT 成功，DELETE 主表 row 失败 → dump 和主表都有这部分 row，查询合并去重负责。
  - Mitigation：合并去重按 `ParquetFileMeta.id`，保证不重复返回。指标 `parquet_file_meta_dump_partitions_skipped_total{reason="duplicate_id"}` 监控这种情况。
- **dump 索引表自身也会膨胀**：主表搬运到 dump 索引表，dump 索引表的行数 ≈ 分区数 × org 数 × stream 数 × days。
  - Mitigation：dump 索引行本身比 ParquetFileMeta 短一个量级（5-6 列 vs 几十列），且 retention 长跑后是定常增长（每天 +N 条）。如果未来还是膨胀，做 dump_of_dumps 二级压缩。本 change 不处理。
- **冷分区查询慢**：原来 PG 主表覆盖全部 retention，冷分区查询毫秒；改成 dump 后冷分区查询多一次 object_store GET。
  - Mitigation：dump.parquet 命中 `parquet_meta` cache + parquet disk cache 后是本地 NVMe 读，远低于 PG 索引扫成本（在大表上）；用 `parquet_file_meta_dump_query_load_seconds` histogram 监控。

## Migration Plan

1. 合并 schema 迁移（`parquet_file_meta_dump` 表）+ 代码 + spec。
2. CI 跑全套，重点新增 it test："dump → 主表 DELETE → query 跨冷热边界返回所有 ParquetFileMeta"。
3. staging：默认 `enabled = true`，跑 ≥ 7 天观察 dump 速率与查询性能；用 `parquet_file_meta_dump_query_load_seconds` 确认 P95 < 100ms。
4. 生产 release notes：默认开启 + 首次启动可能耗时几小时（取决于历史数据量）+ `enabled = false` 关闭办法。
5. 回滚：`enabled = false` 停 worker；已 dump 数据保留可读；如果需要回灌主表，提供 follow-up 工具。

## Open Questions

- dump GC（清除已无 live ParquetFileMeta 的 dump 文件）是否要在本 change 范围内？倾向 follow-up，先看实际累积速度。
- 是否在 dump 写入路径上对 `min_values` / `max_values` 字段做特殊压缩？这两列在 ParquetFileMeta 里通常 JSON-shaped，可以 dictionary encode。第一版按默认 snappy 压缩，benchmark 后决定。
- dump.parquet 是否需要 tantivy index？dump 自身只是 ParquetFileMeta 列表，查询路径加载后用 in-memory filter，应该不需要。
