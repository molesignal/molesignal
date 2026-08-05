## Context

Tantivy 在 molesignal 里以 sidecar `{object_key}.tantivy.tar.zst` 形式存储（`crates/infra/src/search/tantivy_index.rs`），现有实现已经支持：
- `TantivyArchiveBuilder::try_new` 按 `indexed=true && Utf8|Json` 字段构 tantivy schema、tempdir + MmapDirectory 让 tantivy 正常写文件。
- `commit_and_archive` 把 tempdir 整目录 `tar + zstd` 压缩成单 Vec<u8>。
- `TantivyArchiveOpener::open(&[u8])` 反 zstd → 反 tar → tempdir → `Index::open_in_dir` → `IndexHandle`。
- `TantivyFooter` 名为 footer 实则把整 archive bytes 也存住，`caching::tantivy_footer` cache 实质是整文件 cache。

这条路径的真问题不是写 —— 写 1 次几十 MB 一次性 PUT 没什么压力；问题在读：
- 每次 `IndexHandle` 失效（TTL 或 cache evict）都要重新整下载 + 整解压一遍；
- footer cache 缓的是整 archive bytes，size 跟单文件大小线性相关，capacity 上不去；
- tantivy `Directory` 接口本来支持文件粒度 lazy 加载，整 archive 模式把这个能力全废了。

OpenObserve 用 Apache Iceberg [Puffin v1](https://iceberg.apache.org/puffin-spec/) 解决一样的问题：
1. 把 tempdir 里 tantivy 的每个文件作为 puffin blob 拼接进单一 `.ttv` 对象，blob 头的 `properties` 携带 `blob_tag = 文件相对路径`；额外加一个 `BlobTypes::O2TtvFooterV1` blob 存 tantivy 段元信息 cache。
2. footer = `payload_size(4) + flags(4) + magic(4)`，固定 12 字节在文件末尾。
3. reader 端：head 拿 size → `get_range(size - FOOTER_SIZE..size)` → 拿到 payload_size → `get_range` 再读 payload → 解析所有 blob 的 `(offset, length, blob_tag)` → 构造 `PuffinDirReader`，实现 `tantivy::Directory`；tantivy 后续 `read_bytes` / `read_bytes_async` 转成对应 blob 的 sub-range `get_range`。
4. footer + payload 一起缓存（`FOOTER_DATA_CACHE`），re-open `IndexHandle` 不再 IO。

本 change 在 molesignal 上完整复刻这一套，并改 sidecar key 命名跟 OpenObserve 对齐。

## Goals / Non-Goals

**Goals:**
- 把 tantivy 索引格式从 `tar+zstd` 单 blob 变成 Puffin 多 blob，读端走 footer + 按需 blob range read，单次 IndexHandle 加载从 "整 archive bytes" 降到 "几 KB footer + 实际访问的 blob"。
- footer cache 减重：从整文件改成 footer + meta payload（典型 几 KB ~ 几十 KB），capacity 默认上调一个量级。
- sidecar key 迁到 `files/{org}/index/{stream}_{stream_type}/YYYY/MM/DD/HH/{ksuid}.ttv`，与 OpenObserve 风格对齐，便于未来从 OpenObserve 工具链复用 list/lifecycle。
- Puffin / Directory 适配代码隔离到独立 crate `tantivy_utils`，避免污染 infra；具备未来独立演进/复用空间。

**Non-Goals:**
- 不向后兼容旧 `.tantivy.tar.zst` 对象（用户已确认 clean break）；reader 不嗅探 magic 后回退到旧路径。
- 不做 archive 内部压缩：Puffin spec 允许 blob 级 lz4/zstd，本 change 沿用 OpenObserve 当前做法 `compression_codec = None`（tantivy 段文件本身已 compact，再压缩 CPU 收益小）。
- 不复用 OpenObserve `tantivy_utils` 上游代码（license/版本风险）——我们重写一份 puffin spec 实现。spec 仅 ~300 行。
- 不动 `TantivyPruner::prune` 的算子契约；只换底层 IndexHandle 加载路径。
- 不引入新的 query/SQL 语法。

## Decisions

### D1 — 独立 crate `crates/tantivy_utils`

模块：

```
crates/tantivy_utils/
  Cargo.toml
  src/
    lib.rs
    puffin/
      mod.rs          (PuffinMeta, BlobMetadata, MAGIC=PFA1, FOOTER_SIZE=12, PuffinFooterFlags)
      writer.rs       (PuffinBytesWriter)
      reader.rs       (PuffinBytesReader, parse_footer, read_blob_bytes)
    puffin_directory/
      mod.rs          (constants: ALLOWED_FILE_EXT, FOOTER_CACHE, EMPTY_PUFFIN_*)
      writer.rs       (PuffinDirWriter: tantivy::Directory + to_puffin_bytes)
      reader.rs       (PuffinDirReader: tantivy::Directory + PuffinSliceHandle)
      footer_cache.rs (build_footer_cache, FOOTER_DATA_CACHE)
      caching_directory.rs (optional: 与 caching::parquet_meta 集成)
    key_mapping.rs    (convert_parquet_file_name_to_tantivy_file)
```

依赖：`tantivy`, `bytes`, `object_store`, `async_trait`, `bitflags`, `serde`, `serde_json`, `anyhow`, `tempfile`, `hashbrown`, `futures`, `tokio`。**不依赖 `crates/infra`**（避免循环）；reader 通过 `&dyn ObjectStore` 注入对象存储能力。

`infra` 通过 trait 注入读 IO；具体做法：`PuffinBytesReader::new(store: Arc<dyn object_store::ObjectStore>, location: object_store::path::Path, size: u64)` —— 不携带 molesignal 自定义类型，纯依赖 `object_store` crate。

**Why 独立 crate**：用户决策；puffin 这套是通用基建，将来可能想用 puffin 存别的非 tantivy 索引或 caching 层；与 OpenObserve 工程拆分一致。

### D2 — 文件结构与磁盘布局

每个 sidecar `.ttv` 文件：

```
[MAGIC=PFA1 (4)][BLOB_0 bytes][BLOB_1 bytes]…[BLOB_N bytes][MAGIC=PFA1 (4)][PAYLOAD: JSON PuffinMeta (variable)][PAYLOAD_SIZE (4 LE)][FLAGS (4 LE)][MAGIC=PFA1 (4)]
```

- 前 4 字节 `MAGIC=PFA1`（OpenObserve 也用这个标识，方便后期工具链对齐）。
- 中间一连串 blob 字节（按 offset 顺序写入）。
- footer 内圈再次 MAGIC + JSON-encoded `PuffinMeta { blobs: Vec<BlobMetadata>, properties: HashMap }`，后跟 payload size + flags + 末尾 MAGIC。
- 每个 `BlobMetadata` 含 `{ blob_type, fields, snapshot_id, sequence_number, offset, length, compression_codec, properties: { "blob_tag": "<tantivy-relative-path>" } }`。
- `BlobTypes::O2TtvV1` = tantivy 文件 blob；`O2TtvFooterV1` = 我们自己写入的 segment_meta cache blob（用于 reader 重建 segment 元信息时不走 tantivy 全量读）。

footer 解析顺序（参考 OpenObserve）：
1. `get_range(size - FOOTER_SIZE..size)` 拿 12 字节，校验末尾 MAGIC，解析 payload_size + flags。
2. `get_range(size - FOOTER_SIZE - payload_size..size - FOOTER_SIZE)` 拿到 JSON payload + 起始 MAGIC。
3. JSON deserialize 出 `PuffinMeta`。
4. 通过 `BlobMetadata.properties["blob_tag"]` 把 blob 关联到 tantivy 文件名 PathBuf。

### D3 — `PuffinDirReader` 实现 `tantivy::Directory`

关键接口：

```rust
impl Directory for PuffinDirReader {
    fn get_file_handle(&self, path: &Path) -> Result<Arc<dyn FileHandle>, OpenReadError> {
        let meta = self.blobs.get(path).ok_or(OpenReadError::FileDoesNotExist(...))?;
        Ok(Arc::new(PuffinSliceHandle { source: self.source.clone(), metadata: meta.clone() }))
    }
    fn atomic_read(&self, _path) -> Result<Vec<u8>, OpenReadError> { unimplemented!("read-only") }
    fn open_write(&self, _path) -> … { unimplemented!("read-only") }
    fn exists(&self, path) -> Result<bool, OpenReadError> { Ok(self.blobs.contains_key(path)) }
    fn watch(&self, _) -> tantivy::Result<WatchHandle> { Ok(WatchHandle::empty()) }
    fn sync_directory(&self) -> io::Result<()> { unimplemented!("read-only") }
}

#[async_trait]
impl FileHandle for PuffinSliceHandle {
    async fn read_bytes_async(&self, byte_range: Range<usize>) -> io::Result<OwnedBytes> {
        let absolute = self.metadata.offset + byte_range.start as u64
                    .. self.metadata.offset + byte_range.end as u64;
        let bytes = self.source.get_range(absolute).await?;
        Ok(OwnedBytes::new(bytes.to_vec()))
    }
    fn read_bytes(&self, _) -> io::Result<OwnedBytes> { Err(io::Error::other("sync not supported")) }
}
```

特殊文件（`meta.json` / segment file）— OpenObserve 用一份 "empty puffin directory" 兜底：当 tantivy 请求 `.fast` / `.fieldnorm` 等 segment 文件但 puffin 里没有时，从内嵌的"空目录"返回。我们沿用同一思路，把空 segment 文件的字节作为 const 静态资源嵌入 crate（一次性生成，~几 KB）。

### D4 — `PuffinDirWriter` 实现写路径

继承 OpenObserve 思路：

```rust
pub struct PuffinDirWriter {
    mmap: Arc<MmapDirectory>,             // tempdir，让 tantivy 正常写
    file_paths: Arc<RwLock<HashSet<PathBuf>>>,
    properties: Arc<RwLock<HashMap<String, String>>>,
}
impl Directory for PuffinDirWriter { /* 全部 delegate 到 mmap */ }
impl PuffinDirWriter {
    pub fn to_puffin_bytes(&self) -> Result<Vec<u8>> {
        let mut writer = PuffinBytesWriter::new(...);
        for path in self.allowed_files() {
            let bytes = self.mmap.open_read(path)?.read_bytes()?;
            writer.add_blob(&bytes, BlobTypes::O2TtvV1, /* blob_tag */ path.to_string_lossy());
        }
        let footer_cache = build_footer_cache(self.mmap.clone())?;
        writer.add_blob(&footer_cache, BlobTypes::O2TtvFooterV1, FOOTER_CACHE.to_string());
        writer.finish()
    }
}
```

`ALLOWED_FILE_EXT` 沿用 OpenObserve 范围（`.term`, `.idx`, `.pos`, `.store`, `.fast`, `.fieldnorm`, `.del`, `.json`, `.lock`），不在白名单的文件不进 blob，防止 tantivy 内部临时文件污染输出。

### D5 — sidecar object key 迁移

- 旧：`{parquet_object_key}.tantivy.tar.zst`，例如 `orgA/logs/log_app/2026-01-15/abc123.parquet.tantivy.tar.zst`。
- 新：`files/{org}/index/{stream}_{stream_type}/YYYY/MM/DD/HH/{ksuid}.ttv`，例如 `files/orgA/index/log_app_logs/2026/01/15/00/abc123.ttv`。

molesignal 当前 parquet key 模式（`ARCHITECTURE.md Storage layout`）：

```
{org}/{stream_type}/{stream}/{YYYY-MM-DD}/{ksuid}.parquet
```

转换：

```rust
pub fn convert_parquet_file_name_to_tantivy_file(parquet_key: &str) -> Option<String> {
    let parts: Vec<&str> = parquet_key.split('/').collect();
    // 期待: [org, stream_type, stream, YYYY-MM-DD, ksuid.parquet]
    if parts.len() != 5 { return None; }
    let [org, stream_type, stream, date, file] = [parts[0], parts[1], parts[2], parts[3], parts[4]];
    let stem = file.strip_suffix(".parquet")?;
    let (y, m, d) = parse_ymd(date)?;
    Some(format!("files/{org}/index/{stream}_{stream_type}/{y:04}/{m:02}/{d:02}/00/{stem}.ttv"))
}
```

由于 molesignal partition 粒度是 day（不像 OpenObserve 是 hour），hour 段固定 `00`。这保留了未来 hourly partition 的扩展空间。

### D6 — Footer cache 重定义

旧 `TantivyFooter`：

```rust
pub struct TantivyFooter {
    pub archive_bytes: bytes::Bytes,   // 整 archive 字节
    pub schema: Schema,
}
```

新形态：

```rust
pub struct TantivyFooter {
    pub puffin_meta: Arc<PuffinMeta>,            // 解析后的 footer JSON：blobs + properties
    pub footer_payload_bytes: bytes::Bytes,      // 几 KB；用于 IndexHandle 重建时无需回源
    pub schema: tantivy::schema::Schema,         // 解析后 tantivy schema
}
```

`size_bytes` 用于 cache eviction：返回 `puffin_meta` JSON 序列化长度 + `footer_payload_bytes.len()` + `schema` 内存估算。capacity 默认从 `10_000`（按整 archive 10 KB 假设当时已经过激进） 仍保留；但因为每个 entry 缩到几 KB 量级，10 万级 entries 也不过几百 MB，因此考虑把 `[cache.tantivy_footer] capacity` 默认从 `10_000` 调到 `100_000` 反映新形态实际容量。

### D7 — 旧 archive 清退

策略：
- reader 拒读 `.tantivy.tar.zst`（不嗅探 magic 兜底）。
- compactor 在 retention 周期内随 parquet 一起 sweep 旧 sidecar 对象（`parquet.deleted = true → 删 sidecar`）。当前 sweep 路径如果没 cover sidecar，要在本 change 加；如果 cover 了，自然消化。
- 提供 one-shot CLI `molesignal-tools tantivy-backfill <org/stream/...>`：列出所有有 `.tantivy.tar.zst` 的 parquet，下载旧 archive → 反 untar → 用新 `PuffinDirWriter` 重写成 `.ttv` → 删旧 sidecar。运维侧选择跑或不跑。

**Why 不内置自动 backfill**：用户已确认 clean break；一旦 reader 切到 puffin 就读不到旧 archive，但因为 `tantivy_missing_archive_total` 会触发 fallback 到 row-by-row MATCH，查询不会失败，只是慢。运维可决定容忍降级或主动 backfill。

## Risks / Trade-offs

- **[Risk] reader 端 lazy IO 数量增加**：tantivy 一次 query 可能触发 N 次 sub-range `get_range`（每个 segment 文件 1-2 次），对低延迟对象存储（local/local-S3）有 RTT 累积影响。**Mitigation**：footer cache 命中可避免 IndexHandle 重建；result cache 命中可避免 tantivy 调用本身；对延迟敏感场景启用 `caching::parquet_disk_cache` 把 `.ttv` 拉到本地一次后续 range read 走 disk（disk cache 已有，本 change 不动）。
- **[Risk] empty puffin directory 静态资源跨 tantivy 版本可能不兼容**：未来 tantivy 升级新增 segment 文件类型时，硬塞的"空文件"可能与新 tantivy 期待不符。**Mitigation**：crate 暴露 `fn rebuild_empty_directory()` 在构建脚本里跑一次，把当前 tantivy 版本下的空文件集合烘焙进 crate；tantivy 版本升级时 CI 检测 const 与新版本是否一致。
- **[Risk] `BlobMetadata.compression_codec` 当前固定 None；未来若启用 zstd 会破坏现有 reader**：**Mitigation**：reader 当前对 `Some(Zstd)` 直接返错（OpenObserve 也这样），等到真要启用时引入版本号 bump 或开关。
- **[Risk] sidecar key 迁移失败导致查询期发现"找不到索引"批量回退到 full scan**：性能掉，无正确性问题。**Mitigation**：迁移前 dry-run 跑 `key_mapping::convert_…` 在生产 sample 上验证；保留 `tantivy_missing_archive_total` metric 告警阈值（迁移期短期升高可接受，长期不归零需介入）。
- **[Risk] crate 边界让 infra→tantivy_utils 单向依赖编译时间增加**：tantivy 本身编译就慢，独立 crate 多一次编译边界。**Mitigation**：tantivy_utils 不暴露 generic / macro 重的接口；只暴露 `&dyn Directory` 与少量 fn；下游编译影响可接受。
- **[Trade-off] OpenObserve magic `PFA1`**：与 OpenObserve 兼容（理论上 OpenObserve 工具也能读我们的 puffin），但代价是把 magic 跟 OpenObserve 绑定；如果未来 fork 演进格式需要改 magic。**Trade-off 接受**：当前阶段对齐 OpenObserve 收益大于绑定风险。

## Migration Plan

Phase A —— 准备：
1. 在维护窗口前评估存量 `.tantivy.tar.zst` 数量与总大小（运维端 `aws s3 ls --recursive | grep tantivy.tar.zst`）；
2. 决定运维策略：(a) 接受迁移期 fallback 到 row-by-row MATCH（慢，正确），(b) 部署前跑 `molesignal-tools tantivy-backfill` 重写为 `.ttv`，(c) 部署前删除全部旧 sidecar（最简单，下次查询走 row-by-row 直到 ingester 自然补新 `.ttv`）。

Phase B —— 部署：
1. 部署新版本，`tantivy_utils` 编译进二进制。
2. 写端：下一个 ingester flush 起开始写 `.ttv` 到新 sidecar 路径。
3. 读端：所有 query 走 puffin reader；旧 sidecar 不再读。

Phase C —— 验证：
- `tantivy_pruned_files_total` 在跨索引谓词查询里增长。
- `tantivy_missing_archive_total` 短期升高（旧文件未迁移）后趋稳（新 flush 文件全有 puffin sidecar）。
- `cache_tantivy_footer_*` 指标稳定，`hit_ratio > 0.6`（稳态期望）。
- `tantivy_puffin_blob_range_reads_total` 计数符合预期（每次 IndexHandle 重建 ≈ N 个 segment 文件 × 1-2 range reads）。

**Rollback**：回到上一版本即可（旧 reader 仍能读旧 `.tantivy.tar.zst`，新写出的 `.ttv` 会被旧 reader 当作不存在 → fallback row-by-row MATCH）。新 ingester 期间产出的 `.ttv` sidecar 是 dead weight 直到下次升级。

## Open Questions

- **Q1**：`crates/tantivy_utils` 内部要不要把 `PuffinDirReader` 的 `tantivy::Directory` 实现升级成 OpenObserve 的两阶段（`PuffinDirReader` → `CachingDirectory` → tantivy）？OpenObserve `caching_directory.rs` 把 tantivy 多次 read 同一文件做内存 cache，降 sub-range read 次数。我倾向先不做（保最小实现），上线后看 `tantivy_puffin_blob_range_reads_total` 决定。
- **Q2**：empty puffin directory 静态资源是 build.rs 生成还是 manual 入库？build.rs 灵活但 tantivy 起 build script 会跟下游 CI 互动；manual 入库需要在 tantivy 升级时手动跑 helper。**倾向**：先 manual + Cargo doc 说明，build.rs 留 follow-up。
- **Q3**：`crates/tantivy_utils` 是否暴露 `pub` API 给 `crates/infra` 之外？如果未来 web/api 想直接做 inspect 用 puffin reader，跨 crate 调用是否允许？倾向暴露 `pub` 但限制只暴露 Reader/Writer/key_mapping，不暴露 internal helpers。
