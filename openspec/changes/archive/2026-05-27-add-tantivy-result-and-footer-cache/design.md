## Context

`TantivyPruner` 是文件级裁剪的核心，但其工作集中在每条 `MatchPredicate` 都要在每个候选 archive 上调用 `IndexHandle::count_term(field, term)`，而 tantivy term query 即便命中很小的 archive 也有非零的固定成本（schema 解析 + segment reader 初始化 + term dictionary 查表）。同一查询在同一文件上的重复评估（dashboard 自动刷新、相邻时间窗滑动）非常常见。

现状（`crates/infra/src/query/tantivy_pruner.rs`）：

```text
prune(candidates, predicates):
  for fm in candidates:
    handle = cache.get_or_load(archive_key, || fetch_archive_bytes + open)
    for p in predicates:
      n = handle.count_term(p.field, p.term)
      ...
```

唯一的缓存是 `ParquetMetaCache<Arc<IndexHandle>>`，命中时跳过 archive 的下载和打开。但：

- `count_term` 的返回值没有缓存 — 同一 `(archive_key, field, term)` 在 IndexHandle TTL 内重复 query 仍然每次跑。
- IndexHandle TTL 过期后，重新打开必须重新 `object_store.get(archive_key)` 拉整段 bytes（archive 包含完整 tantivy index 数据，通常远大于其 "footer/manifest" 部分）。

OpenObserve 的对应做法：

| OpenObserve 缓存 | 命中场景 | molesignal 对应缺口 |
|---|---|---|
| `tantivy_result_cache::GLOBAL_CACHE` | 相同 IndexCondition + 同一文件 | 同一 `(archive_key, field, term)` 命中 → 跳过 `count_term` |
| `FooterCache::from_directory(...)` | tantivy index 重新 open 时复用 footer | archive bytes 失效时复用 footer/manifest，避免回源 |
| `CachingDirectory::new_with_cacher(...)` | 已 open 的 index 跨查询复用 | molesignal 已有（`ParquetMetaCache<IndexHandle>`） |

## Goals / Non-Goals

**Goals:**

- `TantivyPruner::prune` 路径在 result cache 命中时**完全跳过 tantivy 调用**（不开 archive、不查 term）。
- archive 重新打开路径在 footer cache 命中时跳过整 archive 的 object_store GET，footer/manifest 来自内存。
- 两层 cache 与现有 `[caching]` 三层缓存配置风格一致，独立 capacity / TTL / metrics。
- 写入失效与现有 `ParquetFileMetaRepository::insert/replace/mark_deleted` 共用同一套机制，避免新增冗余路径。
- 默认开启、可关闭、capacity 可调。

**Non-Goals:**

- 不改 tantivy archive 格式或 `IndexHandle` API。
- 不实现 row-level filter cache（molesignal 当前 tantivy 只做 prune，没用到行级 row IDs）。
- 不做跨节点协同（每节点本地 cache 独立）。
- 不替换或合并现有 `ParquetMetaCache<Arc<IndexHandle>>`（它仍然是第一层"已打开 index"缓存）。
- 不实现 parquet disk cache（独立 change `enable-parquet-disk-cache-by-default`）。

## Decisions

### 1. `tantivy_result` cache 的 key 与 value

- Key = `(archive_key: String, field: String, term: String)`，三元组。
- Value = `count: u64`。
- 选用 `moka` LRU + TTL（与现有三层一致）。Default `capacity = 1_000_000`、`ttl_secs = 600`。

**为什么用 count 不是 row IDs**：molesignal 的 `count_term` 返回值就是 count，下游只关心"是否 > 0"。缓存 count 而非更宽的 row IDs 数组让 entry 极小（24-100 B），1M 条目占用约百 MB 量级，合理。

**为什么把 field/term 入 key 而不是缓存 `(archive_key, query_hash)`**：query_hash 需要稳定的序列化方案；`(field, term)` 是 `MatchPredicate` 的全部状态，最简且可读，便于调试。后续如果引入更复杂的 IndexCondition（OR / NOT），再扩展为 `(archive_key, condition_hash)`。

**替代方案**：把缓存层下沉到 `IndexHandle::count_term` 内部（IndexHandle 自带 cache）。否决：增加 `IndexHandle` 的耦合，且无法对"已 evict 的 IndexHandle 但 result 仍有效"的场景生效（result cache 与 handle cache 独立，handle 过期不丢 result）。

### 2. `tantivy_footer` cache 的 key 与 value

- Key = `archive_key: String`。
- Value = `Arc<TantivyFooter>`，一个轻量结构包含 manifest / segment meta / schema bytes 的反序列化结果（具体字段由 `tantivy_index::TantivyArchive` 内部定义；本 change 在 `tantivy_index.rs` 增加 `Footer` 提取方法）。
- Default `capacity = 10_000`、`ttl_secs = 3600`。

**为什么 TTL 更长**：footer/manifest 在 archive 写入后不变（archive 是不可变文件）；TTL 主要用作 GC，不是为了一致性。

**实现路径**：
1. `TantivyArchiveOpener::open(bytes)` 增加 `extract_footer(bytes) -> Footer` 静态方法。
2. 新增 `open_with_footer(footer, body_bytes)` 接受已经反序列化的 footer + 仍需下载的 segment body。
3. `TantivyPruner::load_handle` 首先查 footer cache，命中则只 GET archive 的 body（如果 archive 格式允许 partial read），完全不命中才整段 GET。
4. **现实约束**：molesignal archive 是 tar.zst 整段；partial read 需要解压一遍才能拆 entry。如果 archive 格式不支持 cheap partial read，footer cache 的 win 主要在解析侧（解析 manifest 仍然占可观时间），落地阶段需要 benchmark 决定是否值得在 archive 写入时把 footer 独立成 `.tantivy.meta` 小对象。Open Question 列出。

### 3. 失效模式：跟 parquet_file_meta 同口径

`ParquetFileMetaRepository::insert / replace / mark_deleted` 已经存在按 `(org, stream, stream_type)` 前缀失效的逻辑（见 `caching` capability "Cache Invalidation on Write" requirement）。两个新 cache 沿用同一套：

- Insert：不需要主动失效（新 archive_key 不与已有 entry 冲突）。
- Replace（compactor merge）：被合并掉的 archive_key 对应的 result + footer entry 立刻 `invalidate(archive_key)`，避免在 archive 删除后仍命中过期 entry。
- Mark deleted（retention）：同 Replace 处理。

**替代方案**：完全不主动失效，靠 TTL 自然过期。否决：retention 删 archive 后 cache 在 TTL 内仍可能返回 count > 0，让上游 pruner 决定"保留"这个已删 archive，下一步真正读 archive 时 NotFound。功能不会出错（pruner 容错），但白白浪费判断。主动失效成本低，值得做。

### 4. 指标命名与位置

- `cache_tantivy_result_hits_total` / `misses_total` / `evictions_total` (Counter) + `cache_tantivy_result_hit_ratio` (Gauge)。
- `cache_tantivy_footer_hits_total` / `misses_total` / `evictions_total` (Counter) + `cache_tantivy_footer_hit_ratio` (Gauge)。
- 注册位置：与现有 `cache_parquet_file_meta_*` 一起，在统一的 metrics 注册模块（已存在 `register_int_counter` / `register_gauge` 辅助函数）。

### 5. 默认值

| 设置 | 默认 | 理由 |
|---|---|---|
| `cache.tantivy_result.capacity` | `1_000_000` | 每 entry 约 24-100 B；1M 占 ~100 MB |
| `cache.tantivy_result.ttl_secs` | `600` | 10 分钟；典型 dashboard 刷新窗口内反复命中 |
| `cache.tantivy_footer.capacity` | `10_000` | archive 数级别远小于 result 数 |
| `cache.tantivy_footer.ttl_secs` | `3600` | 1 小时；archive 不可变，TTL 仅作 GC |

## Risks / Trade-offs

- **result cache 在错误前提下命中**：理论上同一 `(archive_key, field, term)` 在 archive 内容不变下结果不变；archive 是 immutable，所以只要 invalidation 与删除联动正确，不会读到错误结果。
  - Mitigation：在 compactor replace / mark_deleted 路径加显式 `invalidate(archive_key)`；写一条 it test 覆盖"删 archive 后 cache 不返回 stale count"。
- **footer cache 在 archive 格式不支持 cheap partial read 时收益有限**：如果 footer 必须从 tar.zst 整段解压才能拿到，每次 miss 的成本跟整开 archive 差不多。
  - Mitigation：第一轮先以"反序列化结果"为粒度做缓存（即使 bytes 还要重新下载，反序列化结果可以复用），衡量收益。如果不足，作为 follow-up 把 footer 拆为独立 `.tantivy.meta` 对象。
- **内存峰值**：两层 cache 加起来默认上限约百 MB 级。
  - Mitigation：capacity 可调；新增 `caching.disable_all = true` 紧急开关属于现有 `caching` capability 已覆盖的能力（无需本 change 引入）。
- **failure 模式**：cache 自身出错（lock 死锁、 OOM）会阻塞 query。
  - Mitigation：所有 cache 操作以 best-effort 形式包装，单次 cache 错误降级到"绕过缓存直接调 tantivy"，并 `cache_<level>_errors_total` 计数。

## Migration Plan

1. 合并 settings + cache 实现 + spec + tests。
2. CI 跑 `it_tantivy_prune` 全套 + 新增的"删除 archive 后失效"测试。
3. staging 跑 ≥ 1 小时反复查询（含 dashboard 自动刷新），观察 `cache_tantivy_result_hit_ratio` 收敛到 > 0.5。
4. 生产 release notes 提到新增 8 个指标 + 默认开启 + 关闭办法（`capacity = 0`）。
5. 回滚：单点回滚到旧二进制；cache 是 in-process，不留持久化痕迹。

## Open Questions

- footer cache 是否需要在 archive 写入侧把 manifest 单独写为 `.tantivy.meta` 对象？取决于 archive 格式实测能否 cheap partial read。第一轮按"反序列化结果缓存"实施，benchmark 后决定是否升级。
- 当 archive 不存在 (`__missing__`) 时，是否要在 footer cache 里缓存一个"NotFound 标记"避免反复 404 GET？这跟 `TantivyPruner` 现有"找不到 archive 就保留候选"语义有关，建议在 implementation 阶段决定。
- result cache 是否要对"全部 predicates miss → 整 archive 被 prune 掉"这一聚合结果再加一层 cache？短期可不做，靠组合多个 entry 命中达到等价效果。
