## Why

`caching/spec.md` 已声明 Parquet Disk Cache requirement，代码侧 `ParquetDiskCache` 也已经实现（`crates/infra/src/storage/object_production.rs`，`with_disk_cache` 接口齐全），但 `conf/config.toml` 缺 `[cache.disk_cache]` 段、bootstrap 没有 wire 注入、Prometheus 也没有指标 — 这层缓存当前是 dark code，没在生产路径上跑。对标 OpenObserve：本地 NVMe 作为 S3 二级缓存默认开启，热数据查询不发 object store GET，是历史数据查询能"秒回"的关键一环。

## What Changes

- 在 `crates/config/src/settings.rs` 新增 `DiskCacheSettings`（`enabled: bool`，`dir: PathBuf`，`max_size_gb: u32`），并把它挂到 `CachingSettings` 下作为 `disk_cache` 字段。Default：`enabled=true / dir="./data/cache/parquet" / max_size_gb=10`。
- `conf/config.toml` 增加 `[cache.disk_cache]` 默认段并附运维注释（占盘说明、关闭办法、目录可调）。
- `bootstrap` 启动期根据 `cache.disk_cache.enabled` 实例化 `ParquetDiskCache`，自动 `mkdir -p` 缓存目录后通过 `ProductionObjectStore::with_disk_cache` 注入。`enabled=false` 时完全跳过实例化。
- 暴露 4 个 Prometheus 指标：`cache_parquet_disk_hits_total`、`cache_parquet_disk_misses_total`、`cache_parquet_disk_evictions_total`（counter）+ `cache_parquet_disk_hit_ratio`（gauge，按 scrape 计算）。
- `caching/spec.md` 中"Parquet Disk Cache"块的 Scenario 校准为新的默认行为（默认 `enabled=true`，目录自动创建）。

## Capabilities

### New Capabilities

(无)

### Modified Capabilities

- `caching`: Parquet Disk Cache requirement 从"声明但未默认接通"升级为"默认启用、自动建目录、指标可观测"。

## Impact

- **配置**：`conf/config.toml` 新增 `[cache.disk_cache]` 段；现有部署滚动升级不破坏（字段都有 default）。
- **代码**：`crates/config`（新 settings）+ `crates/bootstrap`（wire 注入）+ `crates/infra/src/storage/object_production.rs` 注释更新（不改 ParquetDiskCache 自身实现）。
- **磁盘**：默认在 `./data/cache/parquet` 占用最多 10 GB；可通过 `max_size_gb=0` 或 `enabled=false` 关闭。
- **可观测性**：`/metrics` 新增 4 个时间序列。
- **依赖**：不引入新 crate。
