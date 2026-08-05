# Release Notes Draft — `add-tantivy-result-and-footer-cache`

> 合并到 `CHANGELOG.md` 或下一次 release notes 时使用以下条目。

## Behavior change

- **Tantivy 谓词结果 cache 默认开启**：相同 `(archive_key, field, term)` 的重复
  `TantivyPruner::prune` 调用在 10 分钟 TTL 内直接复用 count，完全跳过 tantivy
  查询。Dashboard 自动刷新、相邻时间窗滑动等场景几乎全命中。
- **Tantivy footer cache 默认开启**：IndexHandle TTL 过期重新打开归档时，先查
  footer cache；命中即用缓存的 archive bytes 重建 handle，**不发对象存储 GET**。
- **Compactor + retention 路径自动失效**：`replace(removed, added)` / 标删 / ghost
  cleanup 这三条路径在数据库提交成功后会同步把 `removed.object_key` 对应的
  `{object_key}.tantivy.tar.zst` 从两层 cache 失效。
- **新增 10 个 Prometheus 指标**（按 `cache_<level>_*` 风格）：
  - `cache_tantivy_result_{hits,misses,evictions,errors}_total` Counter
  - `cache_tantivy_result_hit_ratio` Gauge
  - `cache_tantivy_footer_{hits,misses,evictions,errors}_total` Counter
  - `cache_tantivy_footer_hit_ratio` Gauge

## How to opt out

不希望默认占用进程内存可在 `conf/config.toml` 把 `capacity` 调到 0：

```toml
[cache.tantivy_result]
capacity = 0

[cache.tantivy_footer]
capacity = 0
```

`capacity = 0` 时 cache 整层走 no-op 路径（get 永远 None，insert 是 no-op），
`TantivyPruner::prune` 行为退化到无 cache 时的等价语义。

## Memory budget

- `tantivy_result` 默认 `capacity = 1_000_000`、`ttl_secs = 600`；每 entry 约
  24-100 B，1M 占内存 ~100 MB。
- `tantivy_footer` 默认 `capacity = 10_000`、`ttl_secs = 3600`；value 存完整 archive
  bytes，内存占用与归档大小线性相关。大集群建议把 `capacity` 调小（如 1_000）
  避免占用过多 RSS；后续考虑把 footer 单独写到 S3 让 cache 只缓存反序列化结果。

## Cache invalidation guarantee

`compactor::Compactor::with_tantivy_result_cache` / `with_tantivy_footer_cache`
注入后，下列三条 parquet_file_meta 写路径会同步触发对应 archive_key 的 cache 失效：

1. Compactor merge：被合并掉的旧 archive 失效。
2. Retention sweep：超期被标删的 archive 失效。
3. Ghost cleanup：parquet_file_meta 行存在但对象已不在的 archive 失效。
