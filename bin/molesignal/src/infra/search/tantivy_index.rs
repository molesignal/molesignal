// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Tantivy 倒排索引：与 parquet 同写同存同查。
//!
//! change `tantivy-puffin-migration`：sidecar 从 `tar+zstd` 整目录切到 **Puffin v1**
//! 单文件多 blob。写端 `TantivyArchiveBuilder` 内部仍是 tempdir + `MmapDirectory`，
//! `commit_and_archive` 改走 `PuffinDirWriter::to_puffin_bytes()`。读端
//! `TantivyArchiveOpener::open_from_object_store(store, key, size)` 通过
//! `PuffinDirReader` 把 tantivy 每次文件读转成 sub-range `get_range`。
//!
//! Sidecar 命名也从 `{object_key}.tantivy.tar.zst` 切到
//! `files/{org}/index/{stream_type}/{dataset_kind}/{stream}/YYYY/MM/DD/HH/{id}.ttv`，
//! 由 `tantivy::key_mapping::convert_parquet_file_name_to_tantivy_file` 决定。

use std::{collections::HashMap, sync::Arc};

use anyhow::{Result, anyhow};
use object_store::{ObjectStore, path::Path as ObjectPath};
use tantivy::{
    Index, TantivyDocument, Term,
    collector::Count,
    query::TermQuery,
    schema::{Field, IndexRecordOption, STRING, Schema, TEXT},
};

use crate::{
    domain::stream::{FieldType, StreamDefinition},
    tantivy::puffin_directory::{PuffinDirReader, PuffinDirWriter},
};

const WRITER_HEAP_BYTES: usize = 50_000_000;

// =====================================================================
//  Builder
// =====================================================================

pub struct TantivyArchiveBuilder {
    dir: PuffinDirWriter,
    index: Index,
    writer: tantivy::IndexWriter,
    fields: HashMap<String, Field>,
}

impl TantivyArchiveBuilder {
    /// 按 stream schema 中 `indexed=true && Utf8/Json && !encrypted` 字段建 tantivy schema。
    /// 没有任何可索引字段时返 `Ok(None)`（caller 跳过 tantivy 同写）。
    ///
    /// 字段级选型：`exact=true` 建**未分词** `STRING` 索引（整值单 term，供 `col = 'x'`
    /// 等值裁剪）；否则建**分词** `TEXT`（供 `MATCH()` 全文包含）。二者不可兼得。
    pub fn try_new(stream: &StreamDefinition) -> Result<Option<Self>> {
        let mut sb = Schema::builder();
        let mut fields = HashMap::new();
        for f in &stream.schema.fields {
            if !f.indexed || !matches!(f.data_type, FieldType::Utf8 | FieldType::Json) {
                continue;
            }
            // 加密字段列里是密文，明文查询（等值或全文）必然 miss；索引它只会让 pruner
            // 误裁。直接跳过——加密与「可被文本检索」本就互斥。
            if f.encrypted {
                continue;
            }
            let options = if f.exact { STRING } else { TEXT };
            let tf = sb.add_text_field(&f.name, options);
            fields.insert(f.name.clone(), tf);
        }
        if fields.is_empty() {
            return Ok(None);
        }
        let schema = sb.build();
        let dir = PuffinDirWriter::new().map_err(|e| anyhow!("puffin writer: {e}"))?;
        let index = Index::create(dir.clone(), schema, tantivy::IndexSettings::default())
            .map_err(|e| anyhow!("tantivy index create: {e}"))?;
        let writer: tantivy::IndexWriter = index
            .writer(WRITER_HEAP_BYTES)
            .map_err(|e| anyhow!("tantivy writer: {e}"))?;
        Ok(Some(Self {
            dir,
            index,
            writer,
            fields,
        }))
    }

    /// 把一行追加为 tantivy doc：`field_name → value` 仅对已注册的 indexed 字段生效。
    pub fn add_doc(&mut self, field_values: &HashMap<&str, &str>) -> Result<()> {
        let mut doc = TantivyDocument::default();
        for (name, val) in field_values {
            if let Some(tf) = self.fields.get(*name) {
                doc.add_text(*tf, val);
            }
        }
        self.writer
            .add_document(doc)
            .map_err(|e| anyhow!("tantivy add_document: {e}"))?;
        Ok(())
    }

    pub fn fields(&self) -> &HashMap<String, Field> {
        &self.fields
    }

    /// commit + 把 tantivy 段文件序列化进 Puffin v1 单文件。
    pub fn commit_and_archive(mut self) -> Result<Vec<u8>> {
        self.writer
            .commit()
            .map_err(|e| anyhow!("tantivy commit: {e}"))?;
        drop(self.writer);
        drop(self.index);
        self.dir.set_property("ms_format_version", "1");
        self.dir.to_puffin_bytes()
    }
}

// =====================================================================
//  Opener + Handle
// =====================================================================

pub struct IndexHandle {
    index: Index,
    schema: Schema,
    /// keep reader 引用住 puffin source（即 `Arc<dyn ObjectStore>` + range cache）。
    _puffin_dir: PuffinDirReader,
}

impl IndexHandle {
    pub fn schema(&self) -> &Schema {
        &self.schema
    }

    /// 查询一个 `(field, term)` 在该索引中的命中数（0 → 可剔除）。
    pub fn count_term(&self, field_name: &str, term: &str) -> Result<usize> {
        let field = self
            .schema
            .get_field(field_name)
            .map_err(|e| anyhow!("tantivy field {field_name}: {e}"))?;
        let q = TermQuery::new(Term::from_field_text(field, term), IndexRecordOption::Basic);
        let reader = self
            .index
            .reader()
            .map_err(|e| anyhow!("tantivy reader: {e}"))?;
        let searcher = reader.searcher();
        let count = searcher
            .search(&q, &Count)
            .map_err(|e| anyhow!("tantivy search: {e}"))?;
        Ok(count)
    }
}

pub struct TantivyArchiveOpener;

impl TantivyArchiveOpener {
    /// 从 object_store 读 footer 后构造 IndexHandle。所有后续 tantivy IO 都会转成
    /// 对单 blob 的 sub-range `get_range`，**不下载整 archive**。
    pub async fn open_from_object_store(
        store: Arc<dyn ObjectStore>,
        location: ObjectPath,
        size: u64,
    ) -> Result<IndexHandle> {
        let puffin_dir = PuffinDirReader::from_object_store(store, location, size).await?;
        Self::index_from_dir(puffin_dir)
    }

    /// 用已 cache 的 puffin meta + schema + 预物化 atomic_files 构造 IndexHandle
    /// （footer cache 命中路径，**零 IO**）。
    pub fn open_with_cached_footer(
        store: Arc<dyn ObjectStore>,
        location: ObjectPath,
        size: u64,
        footer: &TantivyFooter,
    ) -> Result<IndexHandle> {
        let puffin_dir = PuffinDirReader::from_cached_footer(store, location, size, footer);
        Self::index_from_dir(puffin_dir)
    }

    fn index_from_dir(puffin_dir: PuffinDirReader) -> Result<IndexHandle> {
        let index = Index::open(puffin_dir.clone()).map_err(|e| anyhow!("tantivy open: {e}"))?;
        let schema = index.schema();
        Ok(IndexHandle {
            index,
            schema,
            _puffin_dir: puffin_dir,
        })
    }
}

// =====================================================================
//  Footer cache value（puffin meta + payload + schema；不含 archive 全字节）
// =====================================================================

pub use crate::tantivy::TantivyFooter;

/// 一份索引的目标 object key。
pub struct TantivyArchive {
    pub object_key: String,
    pub bytes: Vec<u8>,
}

impl TantivyArchive {
    /// 把 parquet object key 转换成对应的 puffin sidecar key（`.ttv` 后缀）。
    ///
    /// 返回 `None` 的兜底：parquet key 不符合规范小时分区时不写 sidecar。
    /// caller 应跳过 Tantivy 写出，避免产生无法由查询路径反向定位的孤儿索引。
    pub fn key_for(parquet_object_key: &str) -> Option<String> {
        crate::tantivy::key_mapping::convert_parquet_file_name_to_tantivy_file(parquet_object_key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::stream::{FieldDef, FieldType, Retention, Schema as DomainSchema},
        shared::{ids::Id, time::TimestampMicros},
    };

    fn stream_with_indexed_message() -> StreamDefinition {
        StreamDefinition {
            id: Id::new(),
            org_id: Id::from_string("org"),
            name: "logs".into(),
            stream_type: crate::domain::stream::StreamType::Logs,
            schema: DomainSchema {
                fields: vec![
                    FieldDef {
                        name: "level".into(),
                        data_type: FieldType::Utf8,
                        nullable: false,
                        indexed: false,
                        encrypted: false,
                        exact: false,
                    },
                    FieldDef {
                        name: "message".into(),
                        data_type: FieldType::Utf8,
                        nullable: false,
                        indexed: true,
                        encrypted: false,
                        exact: false,
                    },
                ],
            },
            retention: Some(Retention { days: 7 }),
            created_at: TimestampMicros::now(),
            updated_at: TimestampMicros::now(),
        }
    }

    #[test]
    fn no_indexed_fields_returns_none() {
        let mut s = stream_with_indexed_message();
        s.schema.fields[1].indexed = false;
        let r = TantivyArchiveBuilder::try_new(&s).unwrap();
        assert!(r.is_none());
    }

    /// exact 字段建 STRING（未分词）索引：整值单 term，含大写/标点的值也能被 `count_term`
    /// 精确匹配——这正是 TEXT 字段会误裁的场景（对照 `count_term_does_not_tokenize...`）。
    #[tokio::test]
    async fn exact_field_indexes_whole_value_as_single_string_term() {
        use object_store::{ObjectStoreExt, memory::InMemory, path::Path as ObjectPath};
        let mut s = stream_with_indexed_message();
        // message 改成 exact（STRING）。
        s.schema.fields[1].exact = true;
        let mut b = TantivyArchiveBuilder::try_new(&s).unwrap().unwrap();
        for msg in &["my-service/API", "other"] {
            let mut v = HashMap::new();
            v.insert("message", *msg);
            b.add_doc(&v).unwrap();
        }
        let bytes = b.commit_and_archive().unwrap();
        let store: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
        let path = ObjectPath::from("exact.ttv");
        store.put(&path, bytes.clone().into()).await.expect("put");
        let handle = TantivyArchiveOpener::open_from_object_store(store, path, bytes.len() as u64)
            .await
            .expect("open");

        // 整值精确匹配命中（含大写和标点，TEXT 分词下会 miss）。
        assert_eq!(handle.count_term("message", "my-service/API").unwrap(), 1);
        // 子串 / 分词后的片段查不到（STRING 不切分 → 等值语义）。
        assert_eq!(handle.count_term("message", "my").unwrap(), 0);
        assert_eq!(handle.count_term("message", "api").unwrap(), 0);
    }

    /// 加密字段即便 indexed=true 也不进 tantivy schema（列里是密文，索引它只会误裁）。
    #[test]
    fn encrypted_indexed_field_is_skipped() {
        let mut s = stream_with_indexed_message();
        s.schema.fields[1].encrypted = true; // message: indexed=true + encrypted=true
        // 只剩加密字段可选 → 无可索引字段 → None。
        assert!(TantivyArchiveBuilder::try_new(&s).unwrap().is_none());
    }

    /// 存量 json 全文索引不受影响：builder 仍为 `indexed=true && Json` 字段建 TEXT 索引
    /// （spec stream-index-config「存量全文索引兼容」——新配置已由 API 层 400 拦下，写侧
    /// 保持 `Utf8 | Json` 原状，json full_text 检索与裁剪行为不变）。
    #[tokio::test]
    async fn json_full_text_field_still_builds_text_index() {
        use object_store::{ObjectStoreExt, memory::InMemory, path::Path as ObjectPath};

        let mut s = stream_with_indexed_message();
        s.schema.fields[1].data_type = FieldType::Json; // message: indexed=true + Json
        let mut b = TantivyArchiveBuilder::try_new(&s)
            .unwrap()
            .expect("json full_text 仍应建 TEXT 索引");
        let mut v = HashMap::new();
        v.insert("message", r#"{"error": "panic at line 1"}"#);
        b.add_doc(&v).unwrap();
        let bytes = b.commit_and_archive().unwrap();

        let store: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
        let path = ObjectPath::from("json.ttv");
        store.put(&path, bytes.clone().into()).await.expect("put");
        let size = bytes.len() as u64;
        let handle = TantivyArchiveOpener::open_from_object_store(store, path, size)
            .await
            .expect("open");
        assert_eq!(handle.count_term("message", "panic").unwrap(), 1);
    }

    #[tokio::test]
    async fn build_and_count_via_puffin_round_trip() {
        use object_store::{ObjectStoreExt, memory::InMemory, path::Path as ObjectPath};
        let s = stream_with_indexed_message();
        let mut b = TantivyArchiveBuilder::try_new(&s).unwrap().unwrap();
        for msg in &["hello panic at line 1", "all good", "another panic here"] {
            let mut v = HashMap::new();
            v.insert("message", *msg);
            v.insert("level", "info");
            b.add_doc(&v).unwrap();
        }
        let bytes = b.commit_and_archive().unwrap();
        assert!(!bytes.is_empty());

        let store: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
        let path = ObjectPath::from("test.ttv");
        store.put(&path, bytes.clone().into()).await.expect("put");
        let size = bytes.len() as u64;
        let handle = TantivyArchiveOpener::open_from_object_store(store, path, size)
            .await
            .expect("open");
        let n = handle.count_term("message", "panic").unwrap();
        assert_eq!(n, 2);
        let n = handle.count_term("message", "banana").unwrap();
        assert_eq!(n, 0);
        // 未索引字段查询应 Err（field 不在 tantivy schema）
        assert!(handle.count_term("level", "info").is_err());
    }

    /// `count_term` 用 `Term::from_field_text` 拿**原始串**去查，而 `TEXT` 字段入索引时
    /// 过了默认分词器（小写化 + 按非字母数字切分）。两侧不一致 → 含标点或大写的 term
    /// 在此层查出 0。
    ///
    /// 这是底层 term 查询的固有行为，不是 bug 本身；真正的漏数据风险在**调用方**：
    /// `TantivyPruner` 若拿这种 term 的 count==0 去裁文件就会误裁。修复在
    /// [`crate::infra::query::tantivy_pruner::can_prune_match_term`] —— 只有原样等于
    /// 索引 token 形态的 term 才进裁剪谓词，其余交 `LIKE` 兜底。本测试固化底层行为，防止
    /// 有人「顺手把 count_term 改成分词」而悄悄改变等值语义。
    #[tokio::test]
    async fn count_term_does_not_tokenize_the_query_term() {
        use object_store::{ObjectStoreExt, memory::InMemory, path::Path as ObjectPath};
        let s = stream_with_indexed_message();
        let mut b = TantivyArchiveBuilder::try_new(&s).unwrap().unwrap();
        let mut v = HashMap::new();
        v.insert("message", "Service my-api FAILED");
        v.insert("level", "info");
        b.add_doc(&v).unwrap();
        let bytes = b.commit_and_archive().unwrap();
        let store: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
        let path = ObjectPath::from("tok.ttv");
        store.put(&path, bytes.clone().into()).await.expect("put");
        let handle = TantivyArchiveOpener::open_from_object_store(store, path, bytes.len() as u64)
            .await
            .expect("open");

        // 分词后的小写单 token 能查到——这也是现有测试唯一覆盖的形态。
        assert_eq!(handle.count_term("message", "failed").unwrap(), 1);
        assert_eq!(handle.count_term("message", "my").unwrap(), 1);

        // 而原样的大写 / 带标点 term 查不到，尽管文档确实含有它：
        assert_eq!(
            handle.count_term("message", "FAILED").unwrap(),
            0,
            "大写 term 查不到（索引里已被小写化）"
        );
        assert_eq!(
            handle.count_term("message", "my-api").unwrap(),
            0,
            "带连字符的 term 查不到（索引里已被切成 my / api）"
        );
    }

    #[test]
    fn key_for_uses_puffin_mapping() {
        assert_eq!(
            TantivyArchive::key_for("orgA/logs/raw/log_app/2026/01/15/09/abc.parquet"),
            Some("files/orgA/index/logs/raw/log_app/2026/01/15/09/abc.ttv".to_string())
        );
        assert_eq!(TantivyArchive::key_for("not-a-key"), None);
    }
}
