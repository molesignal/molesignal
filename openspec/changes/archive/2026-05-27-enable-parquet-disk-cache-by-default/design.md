## Context

`ParquetDiskCache` 已在 `crates/infra/src/storage/` 实现（`object_production.rs` 中通过 `ProductionObjectStore::with_disk_cache(Arc<ParquetDiskCache>)` 注入）。它是一个本地 NVMe 二级缓存：每次从 `ObjectStore::get` 拉 parquet 时先读本地，缺则回源 object store 并把字节写入本地缓存目录，按 LRU 淘汰超过容量上限的条目。配套的 spec 在 `openspec/specs/caching/spec.md` "Parquet Disk Cache" 段已声明默认行为（`enabled=true`，目录 `./data/cache/parquet`，10 GB 上限）。

缺口集中在"接线"：

- `crates/config/src/settings.rs::CachingSettings` 只有 `parquet_file_meta` / `parquet_meta` / `query_result` 三段进程内 cache 配置，没有 `disk_cache` 子结构。
- `conf/config.toml` 的 `[cache.*]` 段同样只有上述三层，没有 `[cache.disk_cache]`。
- `crates/bootstrap` 启动 `ProductionObjectStore` 时未实例化 `ParquetDiskCache`，调用 `with_disk_cache` 的链路是空的。
- `ParquetDiskCache` 的命中 / 淘汰事件没有暴露为 Prometheus 指标，运维无法判断这一层是否生效、命中率是否健康。

对标 OpenObserve `infra/src/cache/file_data/disk.rs`：本地磁盘缓存默认 `ZO_DISK_CACHE_ENABLED=true`，容量默认本地盘 50% 或 500 GB 上限，命中率/淘汰量在 `/metrics` 全程可观测。这层是其历史数据查询保持秒级响应的关键。

## Goals / Non-Goals

**Goals:**

- `ParquetDiskCache` 在默认 `conf/config.toml` 下开箱即用，无需运维额外配置即可命中。
- 容量与目录可通过 `[cache.disk_cache]` 段调整或彻底关闭。
- 缓存目录不存在时自动 `mkdir -p`，启动期不因目录缺失 panic。
- 命中率、淘汰、miss 数全部进 `/metrics`，跟现有 `cache_<level>_*` 风格一致。
- 现有部署滚动升级时不感知差异（settings 全部带 default，TOML 漏写字段也能跑）。

**Non-Goals:**

- 不改 `ParquetDiskCache` 自身实现（已存在，LRU/淘汰策略不动）。
- 不实现 Tantivy footer cache、tantivy result cache 或 parquet_file_meta_dump（拆到独立 change）。
- 不做分布式协同（每个节点本地缓存独立，无 cross-node 失效）。
- 不实现冷启动 prewarm（命中需要靠真实查询累积）。

## Decisions

### 1. Settings 结构：`disk_cache` 挂在 `CachingSettings` 下

新增 `DiskCacheSettings`，作为 `CachingSettings.disk_cache` 字段。Default 值：

| 字段 | 默认值 | 说明 |
|---|---|---|
| `enabled` | `true` | 整层开关；`false` 时 bootstrap 跳过实例化 |
| `dir` | `PathBuf::from("./data/cache/parquet")` | 缓存根目录，启动期自动 `create_dir_all` |
| `max_size_gb` | `10` | LRU 上限，0 也视为关闭（与 `enabled=false` 等价） |

**替代方案**：把 `disk_cache` 提升为顶级 `[disk_cache]` 段。否决：与现有 `[cache.parquet_file_meta]` / `[cache.parquet_meta]` / `[cache.query_result]` 风格冲突；运维心智模型上"all in cache"更清楚。

### 2. Bootstrap wire：早于 ProductionObjectStore::new

bootstrap 顺序：

```text
1. molesignal_config::load(...)        ← 读 conf + env
2. 构造底层 ObjectStore（local / s3 / azure / gcs）
3. if settings.cache.disk_cache.enabled && max_size_gb > 0:
     std::fs::create_dir_all(&settings.cache.disk_cache.dir)?
     let cache = Arc::new(ParquetDiskCache::new(dir, max_size_bytes));
     production_store = ProductionObjectStore::new(inner).with_disk_cache(cache);
   else:
     production_store = ProductionObjectStore::new(inner);
```

**替代方案**：在 `ProductionObjectStore::new` 内部读 `molesignal_config::get()` 自动决定挂不挂。否决：infra crate 不应该依赖 `molesignal_config::get` 全局单例；当前 wire 由 bootstrap 显式注入更清晰，便于测试。

### 3. 指标名称与现有 `cache_<level>_*` 一致

`<level>` = `parquet_disk`：

- `cache_parquet_disk_hits_total` (Counter)
- `cache_parquet_disk_misses_total` (Counter)
- `cache_parquet_disk_evictions_total` (Counter)
- `cache_parquet_disk_hit_ratio` (Gauge，scrape 时计算 `hits / (hits + misses)`)

`ParquetDiskCache` 实现里在 hit / miss / evict 处加 `metrics::counter!` / `metrics::gauge!` 调用；现有 `caching/spec.md` 的 "Cache Metrics Exposed via `/metrics`" requirement 已经预设了这套命名规则，无需 spec 改动。

**替代方案**：用单一 `cache_parquet_disk_total{result="hit|miss|evict"}` 三标签 counter。否决：与现有 `cache_parquet_file_meta_hits_total` 等命名风格不一致，会让 dashboard 拆改成本变大。

### 4. spec 中 Scenario 增加 "默认启用 + 自动建目录"

`caching/spec.md` "Parquet Disk Cache" 块的 Scenario 当前覆盖：命中、淘汰、`enabled=false` 跳过。需要增加：

- "默认启用时，启动期自动创建不存在的缓存目录"
- "max_size_gb=0 等价于 enabled=false"

同时把 "When `[cache.disk_cache] enabled = true`" 改为更精确的"启动期 if `enabled && max_size_gb > 0`"，反映实际 wire 行为。

### 5. 滚动升级兼容

`CacheSettings.disk_cache` 字段用 `#[serde(default)]`，老的 `config.toml` 即使不含 `[cache.disk_cache]` 段也能加载（字段全用 default 值）；老部署升级到新版本会**自动启用**这层缓存。运维如果不希望默认占盘，需要在升级前在自己 `config.toml` 里显式写 `[cache.disk_cache]\nenabled = false`。Release note 需要点出这一行为变化。

## Risks / Trade-offs

- **盘空间静默占用 10 GB**：默认启用后，所有现有部署升级会自动开始填本地盘。
  - Mitigation：Release note 显式提示；`max_size_gb` 可调；提供 `enabled=false` 关闭。
- **缓存目录所在分区写满**：极端情况下可能影响其它服务。
  - Mitigation：依赖 `ParquetDiskCache` 现有 LRU 上限严格按 `max_size_gb` 控制；新增 `cache_parquet_disk_evictions_total` 让运维能观察淘汰速率。文档建议把缓存目录指向独立盘或单独的 quota 控制。
- **多进程同节点冲突**：standalone + 拆 role 模式如果在同一 host 跑多份 molesignal，会共用同一目录互踩。
  - Mitigation：默认目录是项目相对路径 `./data/cache/parquet`，docker compose / k8s 部署天然隔离。同主机多进程的少见场景由运维显式配不同 `dir`。
- **cache poison（缓存里残留损坏 parquet）**：极端情况下读出的 bytes 解析失败。
  - Mitigation：依赖 `ParquetDiskCache` 内部对落盘 bytes 的完整性校验（如已实现）；如未实现作为 follow-up，不属于本 change 范围。

## Migration Plan

1. 合并 settings + bootstrap + spec 改动到主干。
2. CI 跑全量 unit + integration test，重点确认 `ProductionObjectStore` 注入 `ParquetDiskCache` 后回源逻辑保持不变。
3. 部署到 staging：确认 `/metrics` 出现 `cache_parquet_disk_*`、命中率随时间收敛到 > 0；磁盘占用稳定在 `max_size_gb` 以内。
4. 生产 release notes 显式说明：默认启用、占盘 10 GB、关闭方法。
5. 回滚：单点回滚到旧二进制即可；本地 `./data/cache/parquet` 目录不需要清理（旧版本不会去读它，新版本下次启动会复用）。

## Open Questions

- `max_size_gb=0` 是否等价于 `enabled=false`，还是单独走"不写不读但仍试图回源"的语义？提案里按 "等价 disabled" 处理，需 spec 显式定义。
- 是否在 startup 时打印一条 INFO log（"parquet disk cache: enabled at ./data/cache/parquet, max 10 GB"）方便运维肉眼确认？建议加，不属于 spec 但属于 tasks。
