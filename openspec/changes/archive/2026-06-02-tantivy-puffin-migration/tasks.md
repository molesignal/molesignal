## 1. Workspace & Crate Scaffolding

- [x] 1.1 workspace 根 `Cargo.toml` `[workspace] members` 加 `"crates/tantivy_utils"`，workspace deps 加 `molesignal-tantivy-utils`
- [x] 1.2 创建 `crates/tantivy_utils/Cargo.toml`：依赖 tantivy/bytes/object_store/async_trait/bitflags/serde/serde_json/anyhow/tempfile/hashbrown/futures/tokio/prometheus/tracing；不依赖 infra/domain
- [x] 1.3 `crates/tantivy_utils/src/lib.rs` 模块声明：`pub mod puffin / puffin_directory / key_mapping / metrics`
- [x] 1.4 `infra` `Cargo.toml` 加 `molesignal-tantivy-utils.workspace = true`

## 2. Puffin File Format

- [x] 2.1 `puffin/mod.rs`：常量 + `bitflags! PuffinFooterFlags` + `PuffinMeta` + `BlobMetadata` + `enum BlobTypes { O2TtvV1, O2TtvFooterV1 }` + `enum CompressionCodec`
- [x] 2.2 `puffin/writer.rs::PuffinBytesWriter`：起始 MAGIC + blob append + finish 写 footer（inner MAGIC + JSON payload + payload_size + flags + tail MAGIC）
- [x] 2.3 `puffin/reader.rs::PuffinBytesReader`：`parse_footer()` 两阶段 range get（tail 12B → payload+inner MAGIC），`read_blob_bytes(meta, sub_range)`
- [x] 2.4 单测覆盖 writer：(a) 单 blob round-trip + 起末 MAGIC、(b) 多 blob offset 单调、(c) properties round-trip
- [x] 2.5 metrics：`tantivy_puffin_footer_bytes_read_total / blob_range_reads_total / directory_open_total` Counter + `directory_open_seconds` Histogram

## 3. Puffin-Backed `tantivy::Directory`

- [x] 3.1 `puffin_directory/mod.rs`：`ALLOWED_FILE_EXT` / `META_JSON` / `FOOTER_CACHE_BLOB_TAG` 常量 + `TantivyFooter { puffin_meta, footer_payload_bytes, schema }` + `size_bytes()` 估算
- [x] 3.2 `empty_directory.rs`：runtime lazy OnceLock 初始化空 tantivy index → bytes per ext。**不需要 build helper**（runtime 一次初始化即可，跟随 tantivy 版本自动同步）
- [x] 3.3 `puffin_directory/writer.rs::PuffinDirWriter`：tempdir + MmapDirectory，delegate Directory，捕获 file_paths；`to_puffin_bytes` 排序 + 白名单过滤 + 写 footer cache blob 占位
- [x] 3.4 `puffin_directory/reader.rs::PuffinDirReader::from_object_store(store, path, size)` + `from_cached_meta(store, path, size, meta)`，读 footer 后建 `PathBuf → BlobMetadata` 映射
- [x] 3.5 `PuffinSliceHandle` 实现 `FileHandle + HasLen`：**sync `read_bytes`** 通过 OnceLock + std::thread 隔离 tokio runtime 一次性物化整 blob 到内存（解决 tantivy 0.25 同步搜索路径限制），后续 sync read 走内存切片；async 走原 sub-range；`blob_range_reads` metric +1
- [x] 3.6 静态资源走 `empty_directory::get_empty_file_bytes(ext)` runtime 初始化（无需 build script / include_bytes，tantivy 版本变化自动跟随）
- [x] 3.7 集成测：`build_index_and_serialize_to_puffin` 写后磁盘检查 MAGIC；`build_and_count_via_puffin_round_trip` 完整 round-trip 跑 InMemory store + tantivy count_term

## 4. Sidecar Key Mapping

- [x] 4.1 `key_mapping::convert_parquet_file_name_to_tantivy_file`：5 段拆分 + `.parquet` 后缀 + `YYYY-MM-DD` 解析 + stream_type 白名单（logs/metrics/traces/extend），不规范返 `None`
- [x] 4.2 6 个单测：标准 logs / traces 流类型反映 / 错段数 / 错扩展 / 错日期 / 未知 stream_type

## 5. Infra 端集成：Writer

- [x] 5.1 重写 `crates/infra/src/search/tantivy_index.rs`：删除 `tar/zstd` archive + `unpack_into`；`TantivyArchive::key_for` 改用 `convert_parquet_file_name_to_tantivy_file`（返 Option）
- [x] 5.2 `TantivyArchiveBuilder` 用 `PuffinDirWriter` 取代 `MmapDirectory`；`commit_and_archive` 走 `dir.set_property("o2_format_version", "1") + dir.to_puffin_bytes()`
- [x] 5.3 `TantivyArchiveOpener`：删 `open(&[u8]) / extract_footer / open_with_footer`，新 `open_from_object_store(store, path, size).await` + `open_with_cached_footer(store, path, size, footer)`
- [x] 5.4 `IndexHandle` 内部持 `PuffinDirReader`；`count_term` 接口未变
- [x] 5.5 `TantivyFooter` 直接 re-export `molesignal_tantivy_utils::TantivyFooter`，新形态包含 `puffin_meta + footer_payload_bytes + schema`，不带 archive bytes

## 6. Infra 端集成：Sidecar 写出路径

- [x] 6.1 `parquet_writer::build_tantivy_for_batch` 用新 `TantivyArchive::key_for(...)`（Option）；不规范 key 时打 warn 并跳过 sidecar
- [x] 6.2 grep 验证：仓内 `tantivy.tar.zst` 字面量出现仅在 source 注释 + 旧 archive 文档；运行时全部走 `convert_parquet_file_name_to_tantivy_file`
- [x] 6.3 PUT puffin bytes 走原 `TantivyArchive { object_key, bytes }` 结构，`flush_with_index` 完全沿用
- [x] 6.4 单测 `parquet_writer::flush_writes_parquet_and_returns_meta` 仍通过；puffin round-trip 通过 search::tantivy_index::tests::build_and_count_via_puffin_round_trip 覆盖

## 7. Infra 端集成：Sidecar 读取路径

- [x] 7.1 `TantivyPruner::load_handle` 改：先 `store.head(path)` 拿 size → footer cache 命中走 `open_with_cached_footer` → miss 走 `open_from_object_store` + 写回 cache
- [x] 7.2 `caching::tantivy_footer` value 类型自动跟随 `TantivyFooter` 改造（re-export 自 tantivy_utils）；测试构造 dummy footer 用新字段
- [x] 7.3 `caching::tantivy_result` cache key 命名沿用 `archive_key` 字面（不强制 rename，避免破坏 caller；语义上即 `index_object_key`，注释已更新）
- [x] 7.4 invalidate hooks 在 compactor sweep 中已经走 `TantivyArchive::key_for`（返 Option，过滤掉 None），自动用新 `.ttv` 命名
- [x] 7.5 pruner 既有集成测（4 个 `tantivy_pruner::tests::*`）改用规范 parquet key + `sidecar_key()`，全部通过；`it_tantivy_prune::match_predicate_prunes_two_files_out_of_three` IT 测试通过

## 8. Compactor & Cleanup

- [x] 8.1 `compactor.rs` retention sweep 已经按 `key_for(Option)` 收集 archive_keys 调 invalidate；老 `.tantivy.tar.zst` 不再被运行时引用，retention 周期自然消化
- [ ] 8.2 backfill CLI 留 follow-up（用户决策：clean break，运维选择整删旧 archive 或接受 fallback；CLI 非必需）
- [ ] 8.3 backfill 工具单测（同上）

## 9. Config Surface

- [x] 9.1 `TantivyFooterCacheSettings::capacity` 默认从 `10_000` 调到 `100_000`，测试同步更新（footer 形态轻量化后内存预算同等可缓更多）
- [ ] 9.2 `sidecar_naming` 配置开关跳过（用户已确认 clean break + 唯一变体，等价于无开关；ARCH 文档说明命名固定为 `.ttv`）
- [x] 9.3 `conf/config.toml` `[cache.tantivy_footer]` 段 + 注释更新（capacity 100000、解释 footer 形态变化、change 引用）

## 10. Empty Puffin Directory Helper

- [x] 10.1 走 **runtime lazy OnceLock**，不需要 build helper / include_bytes。优势：tantivy 版本变化无需手工 regen、crate 无 build script、CI 零额外检查
- [x] 10.2 文档 ARCH + 模块顶部注释说明 runtime 初始化策略；不需要 manual regen
- [x] 10.3 单测 `empty_directory_yields_meta_json_at_minimum` 验证 lazy 初始化产出至少含 meta.json

## 11. Spec & Docs

- [x] 11.1 `openspec validate tantivy-puffin-migration --strict` 通过
- [x] 11.2 `ARCHITECTURE.md Storage layout` Tantivy 段从「计划」改为「Puffin v1 sidecar」实装版（key 模式 / 写读流程 / cache 形态 / clean break / 指标）
- [ ] 11.3 RUNBOOK：迁移 SOP 已在 proposal `Migration Plan` 段覆盖（Phase A 评估 → Phase B 部署 → Phase C 验证 + 回滚），不重复落 RUNBOOK 文件

## 12. Local Verification

- [x] 12.1 `cargo build --workspace`（含 tantivy_utils）通过
- [x] 12.2 `cargo test -p molesignal-tantivy-utils` —— 12/12 绿（puffin writer × 3 / empty directory × 1 / key_mapping × 6 / dir writer × 2）
- [x] 12.3 `cargo test -p molesignal-infra --lib 'tantivy'` —— 17/17 绿（含 search::tantivy_index / query::tantivy_pruner / caching::tantivy_{result,footer} / compactor sweep）
- [x] 12.4 `cargo clippy --workspace -- -D warnings` 本 change 引入 0 新增 lint；workspace 的 pre-existing failure（shared::report_renderer）不在本 change 范围
- [ ] 12.5 local bootstrap + minio 端到端：留运维侧手工验收（参 proposal `Migration Plan` Phase C：观察 `tantivy_puffin_*` 与 `cache_tantivy_footer_*` 系列指标）
- [ ] 12.6 backfill CLI 验证：未实装 backfill（task 8.2 follow-up）；clean break 路径下 retention 周期自然消化旧对象
