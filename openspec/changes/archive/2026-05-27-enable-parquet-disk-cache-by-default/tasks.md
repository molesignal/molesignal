## 1. Settings 与 TOML 默认段

- [x] 1.1 在 `crates/config/src/settings.rs` 新增 `DiskCacheSettings { enabled: bool, dir: PathBuf, max_size_gb: u32 }` 结构，含 `Default`（`enabled=true`、`dir="./data/cache/parquet"`、`max_size_gb=10`）并加 `#[serde(default)]`
- [x] 1.2 在 `CachingSettings` 新增 `pub disk_cache: DiskCacheSettings` 字段，`#[serde(default)]` 保证老 TOML 兼容
- [x] 1.3 在 `conf/config.toml` 增加 `[cache.disk_cache]` 默认段并附运维注释（占盘上限、关闭方法、自定义目录）
- [x] 1.4 写一条 settings 反序列化单测：缺省 TOML 时 `disk_cache` 字段全部为默认值

## 2. Bootstrap wire 注入

- [x] 2.1 定位 bootstrap 中构造 `ProductionObjectStore` 的位置（`crates/bootstrap` 启动期）
- [x] 2.2 当 `cache.disk_cache.enabled && max_size_gb > 0` 时：`std::fs::create_dir_all(&dir)?` → `Arc::new(ParquetDiskCache::new(dir, max_bytes))` → `.with_disk_cache(cache)`；否则跳过
- [x] 2.3 在启动期 INFO log 打印 "parquet disk cache: enabled at <dir>, max <N> GB"（启用时）或 "parquet disk cache: disabled"（禁用时）
- [x] 2.4 写一条 bootstrap-level 集成测试：默认配置启动后，`ProductionObjectStore` 持有 `disk_cache: Some(_)`

## 3. Prometheus 指标

- [x] 3.1 在 `ParquetDiskCache` 命中 / miss / 淘汰路径接入 `cache_parquet_disk_hits_total` / `misses_total` / `evictions_total` Counter（与 `cache_parquet_file_meta_*` 同风格）
- [x] 3.2 注册 `cache_parquet_disk_hit_ratio` Gauge，并在 Prometheus scrape callback 中按 `hits / (hits + misses)` 更新（如已有共享 hit_ratio 计算逻辑则复用）
- [x] 3.3 写一条端到端测试：发起两次同一 parquet 读，断言 `/metrics` 显示 `hits_total >= 1` 且 `hit_ratio > 0`

## 4. Spec 与文档收尾

- [x] 4.1 同步把 `openspec/specs/caching/spec.md` 的 "Parquet Disk Cache" 与 "Cache Metrics Exposed via `/metrics`" requirement 按 `specs/caching/spec.md` 中的 MODIFIED 内容更新（archive 阶段自动套用）
- [x] 4.2 在仓库 README 或运维文档中新增一行："默认本地缓存 ./data/cache/parquet（10 GB LRU），可通过 `[cache.disk_cache]` 段调整或关闭"
- [x] 4.3 在 release notes 草稿里点出默认开启 + 占盘默认 10 GB 的行为变化以及 `enabled=false` 关闭方法

## 5. 验收

- [x] 5.1 `cargo test -p molesignal-config -p molesignal-infra -p molesignal-bootstrap` 全绿
- [x] 5.2 staging 跑 ≥ 1 小时连续查询，确认 `cache_parquet_disk_hit_ratio` 收敛到 > 0 且本地盘占用稳定在 `max_size_gb` 以内
- [x] 5.3 在 dev 环境验证 `enabled=false` 时 `ProductionObjectStore` 不持 `disk_cache`，缓存目录不被创建
