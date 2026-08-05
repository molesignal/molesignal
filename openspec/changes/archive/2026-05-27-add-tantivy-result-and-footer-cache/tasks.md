## 1. Settings 与 TOML 默认段

- [x] 1.1 在 `crates/config/src/settings.rs` 新增 `TantivyResultCacheSettings { capacity: u64, ttl_secs: u32 }`（Default `1_000_000` / `600`）与 `TantivyFooterCacheSettings { capacity: u64, ttl_secs: u32 }`（Default `10_000` / `3600`），含 `#[serde(default)]`
- [x] 1.2 在 `CacheSettings` 新增 `tantivy_result` 与 `tantivy_footer` 字段，`#[serde(default)]` 兼容老 TOML
- [x] 1.3 在 `conf/config.toml` 增加 `[cache.tantivy_result]` 与 `[cache.tantivy_footer]` 默认段并附运维注释
- [x] 1.4 写 settings 反序列化单测：缺省 TOML 时两段全部使用默认值；`capacity = 0` 能正确反序列化

## 2. `tantivy_result` cache 实现

- [x] 2.1 在 `crates/infra/src/caching/` 新增 `tantivy_result.rs`，基于 `moka` 实现 `TantivyResultCache { get, insert, invalidate, invalidate_archive_keys }`
- [x] 2.2 修改 `TantivyPruner::prune`：每个 `(archive_key, p.field, p.term)` 先查 cache，命中直接用 count；miss 时调 `IndexHandle::count_term` 后写回
- [x] 2.3 当 `capacity = 0` 时构造一个 no-op cache（hit 永远 None，insert 直接丢），让 `prune` 调用路径不需要分支
- [x] 2.4 写 `it_tantivy_prune` 扩展用例：相同 predicate 第二次执行时 `count_term` 不被调用（通过 mock 或调用计数）

## 3. `tantivy_footer` cache 实现

- [x] 3.1 在 `crates/infra/src/search/tantivy_index.rs` 新增 `TantivyFooter` 结构（manifest + segment meta + schema 的反序列化结果），并提供 `TantivyArchiveOpener::extract_footer(bytes) -> Footer` 与 `open_with_footer(footer, body_bytes) -> IndexHandle` 两个静态方法
- [x] 3.2 在 `crates/infra/src/caching/` 新增 `tantivy_footer.rs`，提供 `TantivyFooterCache { get, insert, invalidate }`
- [x] 3.3 修改 `TantivyPruner::load_handle`：IndexHandle cache miss 时先查 footer cache；footer cache 命中则复用 footer 重建 IndexHandle，footer cache miss 才走完整 `object_store.get + extract_footer + open_with_footer` 路径
- [x] 3.4 写 it test：手动 evict IndexHandle 后再次 prune，断言 footer cache 命中、不发 object_store GET

## 4. 写时失效

- [x] 4.1 在 `Compactor::merge_group` 合并 commit 后调用 `tantivy_result.invalidate_archive_keys(removed_archive_keys)` 与 `tantivy_footer.invalidate_archive_keys(removed_archive_keys)`（ParquetFileMetaRepository trait 当前无 mark_deleted，invalidate 钩在 compactor 的 `replace` 调用点上）
- [x] 4.2 在 `Compactor::retention_sweep` 标删后同样接入两 cache 的 invalidate；ghost cleanup 路径同步处理
- [x] 4.3 写 it test：删 ParquetFileMeta 后下次 prune 不再返回 stale count（`sweep_invalidates_tantivy_caches_for_merged_archives`）

## 5. Prometheus 指标

- [x] 5.1 注册 `cache_tantivy_result_hits_total` / `misses_total` / `evictions_total` Counter 与 `cache_tantivy_result_hit_ratio` Gauge；在 cache 操作路径里 inc
- [x] 5.2 注册 `cache_tantivy_footer_hits_total` / `misses_total` / `evictions_total` Counter 与 `cache_tantivy_footer_hit_ratio` Gauge
- [x] 5.3 注册 `cache_tantivy_result_errors_total` / `cache_tantivy_footer_errors_total` 用于"cache 自身出错降级到直读"路径
- [x] 5.4 写端到端测试：发起两轮同样的 prune，`/metrics` 显示 hits ≥ 1 且 hit_ratio > 0（`all_eight_tantivy_cache_metrics_visible_after_prune`）

## 6. Spec 与文档收尾

- [x] 6.1 同步把 `openspec/specs/caching/spec.md` 按 `specs/caching/spec.md` 中的 ADDED 内容更新（archive 阶段自动套用）
- [x] 6.2 在 release notes 草稿中点出"tantivy result + footer cache 默认开启 + 8 个新指标 + 关闭办法"

## 7. 验收

- [x] 7.1 `cargo test -p molesignal-config -p molesignal-infra` 全绿
- [x] 7.2 在 staging 跑 ≥ 1 小时含 dashboard 自动刷新的查询，确认 `cache_tantivy_result_hit_ratio` > 0.5
- [x] 7.3 在 dev 环境验证 `capacity = 0` 时 prune 仍正确工作（hit ratio = 0、行为与无 cache 一致）（`result_cache_capacity_zero_falls_through_to_tantivy`）
