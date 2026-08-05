# Release Notes Draft — `enable-parquet-disk-cache-by-default`

> 合并到 `CHANGELOG.md` 或下一次 release notes 时使用以下条目。

## Behavior change

- **Parquet 本地磁盘缓存现在默认开启**：所有 parquet 读取在命中本地 NVMe 缓存
  时直接返回，避免回源 object store GET。这对历史数据查询的延迟收敛尤其明显。
- **默认占盘 10 GB**，路径 `./data/cache/parquet`。容量上限按 LRU 严格执行：
  当 `inserts` 让总占用超过 `max_size_gb` 时，最久未用条目会被立即 unlink。
- **滚动升级无配置 break**：老 `config.toml` 不含 `[cache.disk_cache]`
  段时全部字段走默认（`enabled=true / dir="./data/cache/parquet" / max_size_gb=10`）。
- **新指标**：`/metrics` 暴露 `cache_parquet_disk_hits_total`、`cache_parquet_disk_misses_total`、
  `cache_parquet_disk_evictions_total`（Counter）以及 `cache_parquet_disk_hit_ratio`（Gauge）。

## How to opt out

如不希望默认占用本地盘，可在 `conf/config.toml` 显式关闭：

```toml
[cache.disk_cache]
enabled = false
```

或者把容量调到 0（与 `enabled=false` 等价）：

```toml
[cache.disk_cache]
max_size_gb = 0
```

## How to relocate the cache directory

```toml
[cache.disk_cache]
dir = "/mnt/nvme/molesignal/parquet"
max_size_gb = 50
```

启动期会自动 `mkdir -p` 目录，缓存目录不存在不会让进程拒绝启动。建议把目录指向
独立盘或单独的 quota 挂载，避免与 WAL / 对象存储本地落地互踩。
