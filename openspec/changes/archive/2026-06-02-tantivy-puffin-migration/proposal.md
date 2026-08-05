## Why

当前 tantivy 倒排索引用 `tar+zstd` 整目录归档存到 `{object_key}.tantivy.tar.zst`，查询时必须**整下载、整解压到 tempdir** 才能开 tantivy Directory；现有 `TantivyFooter` cache 实际把整 archive bytes 都缓住（`tantivy_index.rs:206`），既不省 IO 也不省解压。tantivy 几十文件的目录形态本来就是按需访问，被强制整下载浪费严重——尤其对几十 MB 级索引。OpenObserve 用 Iceberg Puffin 把多文件目录序列化进单对象并支持 footer + blob 的 range read，是这条路径上工程能拿到的最大优化。

## What Changes

- **BREAKING**: 新建独立 crate `crates/tantivy_utils`（仿 OpenObserve），承载 puffin spec + puffin-backed tantivy Directory；`crates/infra/src/search/tantivy_index.rs` 改为薄壁，对接新 crate。
- **BREAKING**: tantivy 索引归档格式从 `tar + zstd` 切到 **Puffin v1**：单个二进制文件，多 blob（每个 tantivy 文件 = 一个 blob，meta footer = 一个 blob），文件头 4 字节 magic `PFA1`，footer 含 `payload_size + flags + magic`。
- **BREAKING**: sidecar object key 命名从 `{object_key}.tantivy.tar.zst` 迁到 OpenObserve 风格 `files/{org}/index/{stream}_{stream_type}/YYYY/MM/DD/HH/{ksuid}.ttv`；与现有 `files/{org}/{stream_type}/{stream}/YYYY-MM-DD/{ksuid}.parquet` 做 1:1 映射的工具函数加在新 crate。
- **BREAKING**: `caching::tantivy_footer` cache 重定义 —— value 从"整 archive bytes"换成"puffin footer 字节 + 解析后的 BlobMetadata + 解析后的 `tantivy::schema::Schema`"，size 缩减到几 KB；`IndexHandle` cache 仍保留并依赖 footer cache 重建。
- 写端：`TantivyArchiveBuilder::commit_and_archive` 内部用 `PuffinDirWriter::to_puffin_bytes()` 取代 `tar+zstd`；tempdir + tantivy MmapDirectory 流程不变。
- 读端：`TantivyArchiveOpener::open(bytes)` 接口替换为 `from_object_store(store, object_meta)`；返回的 `IndexHandle` 内部持有 `PuffinDirReader`（实现 `tantivy::Directory`），tantivy 的每次 read 自动转换成对单 blob 的 sub-range `get_range`。
- 旧 `*.tantivy.tar.zst` 对象 clean break：reader 不再支持；compactor 在 retention 周期内自然清理；如需立即清理走 one-shot backfill。

## Capabilities

### New Capabilities
- `tantivy-puffin`: Puffin v1 文件格式实现、puffin-backed `tantivy::Directory`、`PuffinDirReader/Writer`、`{key}.parquet → {key}.ttv` 映射工具。落地为 `crates/tantivy_utils`。

### Modified Capabilities
- `storage`: `Tantivy Inverted Index` requirement 改归档格式（puffin）、改 sidecar key、改 directory 加载语义（range read）。
- `caching`: `Tantivy Footer Cache` 改 value 形态（轻量 footer 元信息，不是整 archive）；`Tantivy Result Cache` 与 `Tantivy Cache Metrics` 名称/key 适配（`archive_key` → `index_object_key`）。

## Impact

- **代码**：新增 `crates/tantivy_utils/{Cargo.toml, src/lib.rs, src/puffin/{mod,reader,writer}.rs, src/puffin_directory/{mod,writer,reader,footer_cache,caching_directory}.rs, src/key_mapping.rs}`；重写 `crates/infra/src/search/tantivy_index.rs`；改 `crates/infra/src/storage/{parquet_writer,compactor,object_production}.rs` 中对 sidecar key 的拼接；改 `crates/infra/src/caching/` 中 tantivy footer/result cache 的 key 与 value 类型。
- **Cargo 工作区**：`Cargo.toml` 加 `tantivy_utils`；`infra` `Cargo.toml` 依赖 `tantivy_utils`。
- **对象存储**：现存 `.tantivy.tar.zst` 仍在 bucket 里但读端不再使用；运维侧需要在迁移窗口跑一次 backfill（重新建 puffin index）或等 retention 把旧对象 sweep 掉。
- **指标**：`tantivy_pruned_files_total`、`tantivy_missing_archive_total`（保留）；`cache_tantivy_footer_*` 含义不变但 size_bytes 数量级会大幅下降；新增 `tantivy_puffin_footer_bytes_read_total`、`tantivy_puffin_blob_range_reads_total` 给观察 range-read 行为用。
- **配置**：`[cache.tantivy_footer].capacity` 默认上调（footer 变轻可以缓更多）、`ttl_secs` 不变；新增 `[storage.tantivy].sidecar_naming = "ttv" | "tar_zst"`（仅 `"ttv"` 生效，配置作占位让运维察觉差异，默认 `"ttv"`）。
- **依赖**：可能引入 `bitflags`（puffin footer flags）、`object_store` 已有，不再加新外部 crate。
- **不影响**：parquet_file_meta_dump / cold tier（独立 change `parquet-file-meta-dump-columnar`）、ingest 热路径、查询 SQL 语法。
