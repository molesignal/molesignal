## Why

Tantivy pruner (`crates/infra/src/query/tantivy_pruner.rs`) 是查询路径上裁掉无关 parquet 文件的关键一层：`TantivyPruner::prune` 对每个候选 ParquetFileMeta 调用 `IndexHandle::count_term(field, term)`，命中 0 则剔除。当前唯一的缓存是 `ParquetMetaCache<Arc<IndexHandle>>`，缓存"已经打开"的 tantivy 索引；TTL 内同一 archive 不会被重新打开，但**同一 IndexHandle 上的同一查询 `(field, term)` 仍然每次都执行**，并且 TTL 过期时整个 archive 的 bytes 要重新回源 object_store。

对标 OpenObserve（`service/search/grpc/storage.rs:740-770`）：
- `tantivy_result_cache::GLOBAL_CACHE` 缓存 `(IndexCondition + file)` → 查询结果，相同条件下完全跳过 tantivy 查询，命中即返回。
- `FooterCache::from_directory(puffin_dir, ...)` 缓存 tantivy index footer/manifest，重新打开 index 时不需要回源对象存储拉完整 archive。

两层加起来是 OpenObserve "查 S3 历史数据也能秒回"性能阶梯里最高命中的一档。molesignal 都没有。

## What Changes

- 新增 `tantivy_result` 进程内 cache：key = `(archive_key, field, term)`，value = `count: u64`。`TantivyPruner` 命中时直接返回 count、跳过 `IndexHandle::count_term`。
- 新增 `tantivy_footer` 进程内 cache：key = `archive_key`，value = 打开 archive 所需的轻量元数据（manifest/footer bytes，不缓存全部 tantivy data）。IndexHandle TTL 过期重新打开时优先从 footer cache 读，避免对 `.tantivy.tar.zst` 的整体回源 GET。
- 在 `crates/config/src/settings.rs::CachingSettings` 增加 `tantivy_result: TantivyResultCacheSettings` 与 `tantivy_footer: TantivyFooterCacheSettings`，沿用现有 `capacity` + `ttl_secs` 字段格式。
- 在 `conf/config.toml` 增加对应 `[cache.tantivy_result]` 与 `[cache.tantivy_footer]` 默认段。
- 暴露 `cache_tantivy_result_hits_total / misses_total / evictions_total / hit_ratio` 与 `cache_tantivy_footer_hits_total / misses_total / evictions_total / hit_ratio` 共 8 个指标，命名与现有 `cache_<level>_*` 风格一致。
- `TantivyPruner::prune` 路径接入两个新 cache：先查 result cache → 未命中则 load IndexHandle（可能命中 footer cache 短路）→ 调 `count_term` → 写回 result cache。
- `caching` capability 扩展两个新 Requirement 描述上述两层缓存的行为；写时失效与现有 cache 共享同一套 `ParquetFileMetaRepository::insert/replace/mark_deleted` 失效模式（**针对发生变化的 archive_key 前缀**做 best-effort invalidation）。

## Capabilities

### New Capabilities

(无)

### Modified Capabilities

- `caching`: 增加 `tantivy_result` 与 `tantivy_footer` 两层进程内 cache 的 Requirement，扩展 `Cache Metrics Exposed via /metrics` 的指标命名集合。

## Impact

- **配置**：`conf/config.toml` 新增两段 `[cache.tantivy_*]`。
- **代码**：`crates/config`（新 settings）+ `crates/infra/src/query/tantivy_pruner.rs`（接入 result cache）+ `crates/infra/src/search/tantivy_index.rs`（接入 footer cache，开 archive 时短路）+ `crates/infra/src/caching/` 复用现有 `moka` LRU 基础设施。
- **内存**：新增两层内存 cache，默认 `capacity` 控制在合理范围（result cache 1M 条 / footer cache 10k 条）。
- **可观测性**：`/metrics` 新增 8 个时间序列。
- **依赖**：不引入新 crate（仍然 `moka` + `prometheus`）。
- **行为**：相同 `(file, field, term)` 重复评估在 TTL 内零 tantivy 调用；archive 重新打开在 footer cache TTL 内零 object_store GET。
