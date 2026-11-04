// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Tantivy 倒排索引的 Puffin 编解码与读取。
//!
//! Tantivy Artifact 从 `tar+zstd` 整目录切到 **Puffin v1**
//! 单文件多 blob。写端 `TantivyArchiveBuilder` 内部仍是 tempdir + `MmapDirectory`，
//! `commit_and_archive` 改走 `PuffinDirWriter::to_puffin_bytes()`。读端
//! `TantivyArchiveOpener::open_from_object_store(store, key, size)` 通过
//! `PuffinDirReader` 把 tantivy 每次文件读转成 sub-range `get_range`。
//!
//! 索引对象 key 由 `StorageLayout` 独立生成，并作为显式 Artifact 记录在 FileCatalog；
//! 本模块不接受 Parquet key，也不承担任何索引路径映射。

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use anyhow::{Result, anyhow};
use object_store::{ObjectStore, path::Path as ObjectPath};
use tantivy::{
    Index, ReloadPolicy, Searcher, TantivyDocument, Term,
    index::SegmentComponent,
    query::{AllQuery, BooleanQuery, Query, TermQuery},
    schema::{
        FAST, Field, FieldEntry, FieldType as TantivyFieldType, IndexRecordOption, STORED, STRING,
        Schema, TextFieldIndexing, TextOptions,
    },
    tokenizer::MAX_TOKEN_LEN,
};

use crate::{
    domain::stream::{FieldType, StreamDefinition, StreamIndexType},
    infra::query::json_members,
    tantivy::puffin_directory::{PuffinDirReader, PuffinDirWriter},
};

mod bars;
mod contains;
mod grams;
mod rows;
mod text;
mod values;

pub use bars::{BarQuery, BarTally};
pub use text::{TEXT_TOKENIZER, is_cjk};

const WRITER_HEAP_BYTES: usize = 50_000_000;

/// 按子串计数时整个读进内存的词典文件上限：更大的词典按需读取，免得一次计数占太多内存。
const MAX_RESIDENT_DICTIONARY_BYTES: u64 = 8 * 1024 * 1024;

/// Fast (columnar) field holding each document's row `_timestamp` in microseconds, so time
/// buckets of matching documents can be counted from the archive.
pub const TIMESTAMP_FIELD: &str = "_timestamp";

/// 文档对应的行在 Parquet 文件里的序号（从 0 起，按文件里的行序）：把命中的少数文档换成文件
/// 里的行，只读这些行所在的页。存在文档库里而不是快字段：取几个文档只读它们所在的块，快字段
/// 却要整列读入。
pub const ROW_FIELD: &str = "_row";

/// 值长到没能建索引的整值字段：文档在这个字段上记下这些字段的名字。
const OVERLONG_FIELD: &str = "_overlong";

/// 新写的归档的格式，也记在 Catalog 里这个索引 Artifact 的 `format_version` 上：格式更旧的索引
/// 由后台重建。格式 4 起分词字段按 [`TEXT_TOKENIZER`] 分词（中日韩文字切成二元组），只记文档、
/// 不记词频与位置。
pub const FORMAT_VERSION: u32 = 4;
const FORMAT_PROPERTY: &str = "ms_format_version";

/// 格式 2 起有 [`ROW_FIELD`] 与 [`OVERLONG_FIELD`]。
const ROWS_VERSION: u32 = 2;

/// 格式 3 起分词字段的长词记下了三元组，见 [`grams`]。
const LONG_GRAMS_VERSION: u32 = 3;

/// 分词字段长词三元组的归档属性名。
fn long_grams_property(field: &str) -> String {
    format!("ms_long_grams:{field}")
}

/// 归档自己用的字段名；数据流里同名的字段不进索引。
const RESERVED_FIELDS: [&str; 3] = [TIMESTAMP_FIELD, ROW_FIELD, OVERLONG_FIELD];

/// JSON 字段的键值索引：在归档里的字段名与词的写法，与查询侧共用。
pub use crate::infra::query::parser::{
    KEY_VALUE_SUFFIX, key_term, key_value_field, key_value_term,
};

// =====================================================================
//  Builder
// =====================================================================

pub struct TantivyArchiveBuilder {
    dir: PuffinDirWriter,
    index: Index,
    writer: tantivy::IndexWriter,
    fields: HashMap<String, Field>,
    /// 按整值建索引的字段。
    whole: HashSet<Field>,
    /// JSON 字段的键值索引：每行第一层的每个键记两个词，见 [`key_value_term`] 与 [`key_term`]。
    members: HashSet<Field>,
    /// 分词字段，各自长词的三元组。
    tokenized: HashMap<Field, grams::LongGrams>,
    timestamp: Field,
    row: Field,
    overlong: Field,
    /// 已加入的文档数，即下一个文档的行号。
    rows: u64,
}

impl TantivyArchiveBuilder {
    /// 按 stream schema 中 `full_text/exact && Utf8/Json && !encrypted` 字段建 Tantivy
    /// schema；别的索引类型不进入倒排 Artifact。
    /// 没有任何可索引字段时返 `Ok(None)`（caller 跳过 tantivy 同写）。
    ///
    /// 字段级选型：`exact` 建**未分词** `STRING` 索引（整值单 term，供 `col = 'x'` 等值裁剪）；
    /// `full_text` 按 [`TEXT_TOKENIZER`] **分词**（供 `MATCH()` 与 `LIKE` 的子串裁剪）。二者不可
    /// 兼得。
    pub fn try_new(stream: &StreamDefinition) -> Result<Option<Self>> {
        let mut sb = Schema::builder();
        let mut fields = HashMap::new();

        let mut whole = HashSet::new();
        let mut members = HashSet::new();
        let mut tokenized = HashMap::new();

        for f in &stream.schema.fields {
            if !matches!(f.data_type, FieldType::Utf8 | FieldType::Json)
                || RESERVED_FIELDS.contains(&f.name.as_str())
            {
                continue;
            }

            // 加密字段列里是密文，明文查询（等值或全文）必然 miss；索引它只会让 pruner
            // 误裁。直接跳过——加密与「可被文本检索」本就互斥。
            if f.encrypted {
                continue;
            }

            let tf = match f.effective_index_type() {
                StreamIndexType::Exact => {
                    let tf = sb.add_text_field(&f.name, STRING);
                    whole.insert(tf);
                    tf
                }
                StreamIndexType::FullText => {
                    let tf = sb.add_text_field(&f.name, text_options());
                    tokenized.insert(tf, grams::LongGrams::default());
                    tf
                }
                StreamIndexType::KeyValue => {
                    let tf = sb.add_text_field(&key_value_field(&f.name), STRING);
                    members.insert(tf);
                    tf
                }
                _ => continue,
            };

            fields.insert(f.name.clone(), tf);
        }

        if fields.is_empty() {
            return Ok(None);
        }

        let timestamp = sb.add_i64_field(TIMESTAMP_FIELD, FAST);
        let row = sb.add_u64_field(ROW_FIELD, STORED);
        let overlong = sb.add_text_field(OVERLONG_FIELD, STRING);
        let schema = sb.build();
        let dir = PuffinDirWriter::new().map_err(|e| anyhow!("puffin writer: {e}"))?;
        let index = Index::create(dir.clone(), schema, tantivy::IndexSettings::default())
            .map_err(|e| anyhow!("tantivy index create: {e}"))?;

        index
            .tokenizers()
            .register(TEXT_TOKENIZER, text::text_analyzer());

        let writer: tantivy::IndexWriter = index
            .writer(WRITER_HEAP_BYTES)
            .map_err(|e| anyhow!("tantivy writer: {e}"))?;

        Ok(Some(Self {
            dir,
            index,
            writer,
            fields,
            whole,
            members,
            tokenized,
            timestamp,
            row,
            overlong,
            rows: 0,
        }))
    }

    /// 把文件的下一行追加为 tantivy doc：`field_name → value` 仅对已注册的 indexed 字段生效；
    /// `timestamp` 是该行的 `_timestamp`（微秒），写入 [`TIMESTAMP_FIELD`] 快字段。行必须按
    /// 它们在 Parquet 文件里的顺序加入，[`ROW_FIELD`] 记的就是这个顺序。
    pub fn add_doc(
        &mut self,
        timestamp: Option<i64>,
        field_values: &HashMap<&str, &str>,
    ) -> Result<()> {
        let mut doc = TantivyDocument::default();
        doc.add_u64(self.row, self.rows);
        self.rows += 1;

        if let Some(timestamp) = timestamp {
            doc.add_i64(self.timestamp, timestamp);
        }

        for (name, val) in field_values {
            let Some(tf) = self.fields.get(*name) else {
                continue;
            };

            if self.members.contains(tf) {
                add_members(&mut doc, *tf, val);
                continue;
            }

            // Tantivy 会丢掉过长的 term；记下来，按值计数时就知道这个文件数不全。
            if self.whole.contains(tf) && val.len() > MAX_TOKEN_LEN {
                doc.add_text(self.overlong, name);
            } else {
                doc.add_text(*tf, val);
            }

            if let Some(grams) = self.tokenized.get_mut(tf) {
                grams.add_value(val);
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
    pub fn commit_and_archive(self) -> Result<Vec<u8>> {
        self.commit()?.to_puffin_bytes()
    }

    /// 同 [`Self::commit_and_archive`]，但把归档写进本地文件 `path`，返回字节数：
    /// 归档不经过内存，再大的索引也能按分片上传。
    pub fn commit_and_archive_to(self, path: &std::path::Path) -> Result<u64> {
        let dir = self.commit()?;

        let file =
            std::fs::File::create(path).map_err(|e| anyhow!("create {}: {e}", path.display()))?;

        dir.write_puffin(std::io::BufWriter::new(file))
    }

    fn commit(mut self) -> Result<PuffinDirWriter> {
        self.writer
            .commit()
            .map_err(|e| anyhow!("tantivy commit: {e}"))?;

        drop(self.writer);
        drop(self.index);
        self.dir
            .set_property(FORMAT_PROPERTY, FORMAT_VERSION.to_string());

        for (name, field) in &self.fields {
            if let Some(grams) = self.tokenized.get(field).filter(|grams| grams.seen()) {
                self.dir
                    .set_property(long_grams_property(name), grams.encode());
            }
        }

        Ok(self.dir)
    }
}

// =====================================================================
//  Opener + Handle
// =====================================================================

pub struct IndexHandle {
    index: Index,
    schema: Schema,
    /// keep reader 引用住 puffin source（即 `Arc<dyn ObjectStore>` + hot cache）。
    puffin_dir: PuffinDirReader,
}

impl IndexHandle {
    pub fn schema(&self) -> &Schema {
        &self.schema
    }

    /// 本句柄已读到的 footer 状态，供 footer cache 保存；不发 IO。
    pub fn footer(&self) -> TantivyFooter {
        self.puffin_dir.footer(self.schema.clone())
    }

    /// 归档的格式，见 [`FORMAT_VERSION`]；没有记下格式的归档是 1。
    fn format_version(&self) -> u32 {
        self.puffin_dir
            .property(FORMAT_PROPERTY)
            .and_then(|version| version.parse().ok())
            .unwrap_or(1)
    }

    /// 同一个归档、各段快字段文件已整个读进内存的句柄；`dictionaries` 时不太大的词典文件也一起读
    /// 进来。按时间分柱、按值计数要读每个命中文档的行时间；按子串数行要走遍分词字段的词典，词典
    /// 逐块读取在对象存储上是一串往返。一轮并发读取代替这些串行的小读取。[`TIMESTAMP_FIELD`] 是
    /// 归档里唯一的快字段，快字段文件几乎就是它本身。为一次计算而建，用完即弃。
    pub async fn with_resident(&self, dictionaries: bool) -> Result<IndexHandle> {
        let segments = self
            .index
            .searchable_segment_metas()
            .map_err(|e| anyhow!("tantivy segments: {e}"))?;

        let mut paths = segments
            .iter()
            .map(|segment| segment.relative_path(SegmentComponent::FastFields))
            .collect::<Vec<_>>();

        if dictionaries {
            paths.extend(
                segments
                    .iter()
                    .map(|segment| segment.relative_path(SegmentComponent::Terms))
                    .filter(|path| {
                        self.puffin_dir
                            .blob_len(path)
                            .is_some_and(|len| len <= MAX_RESIDENT_DICTIONARY_BYTES)
                    }),
            );
        }

        let puffin_dir = self.puffin_dir.with_resident(&paths).await?;
        TantivyArchiveOpener::index_from_dir(puffin_dir)
    }

    /// 整值字段等于 `term` 的文档数，即对应文件里 `field = 'term'` 的行数（0 → 可剔除）。字段
    /// 不在本归档里、或在本归档里不是整值索引时为 `None`：分词字段的词是小写的片段，拿原值去查
    /// 只会查出 0。字段从全文改成精确以后，改之前写下的归档就是这样。
    ///
    /// 归档没有删除，命中数就是词典里的文档频次：只读词典，不读倒排表。每次调用新开
    /// segment reader：打开时要读的 footer 都在 hot cache 里，不发 IO；reader 用完即弃，
    /// 句柄缓存不会长期持有全文字段的大词典。hot cache 未覆盖的读会同步访问对象存储，
    /// 调用方须在 blocking 线程上调用。
    pub fn count_term(&self, field_name: &str, term: &str) -> Result<Option<usize>> {
        let Some(field) = self.whole_value_field(field_name) else {
            return Ok(None);
        };

        let q = TermQuery::new(Term::from_field_text(field, term), IndexRecordOption::Basic);

        q.count(&self.searcher()?)
            .map(Some)
            .map_err(|e| anyhow!("tantivy count: {e}"))
    }

    /// `terms` 全部命中、`contains` 的子串都出现的文档数，即该归档对应文件里 `field = 'value'`
    /// 与 `field ILIKE '%term%'` 同时成立的行数（每行一个文档）。某个等值条件的字段在本归档里
    /// 不是整值索引（分词索引的命中数不等于等值过滤的行数）、某个子串数不出来（见
    /// [`Self::contains_query`]）时返回 `None`。调用方须在 blocking 线程上调用，见
    /// [`Self::count_term`]。
    pub fn count_exact(
        &self,
        terms: &[(String, String)],
        contains: &[(String, String)],
    ) -> Result<Option<usize>> {
        if terms
            .iter()
            .any(|(name, _)| self.whole_value_field(name).is_none())
        {
            return Ok(None);
        }

        let searcher = self.searcher()?;

        let Some(query) = self.matching_query(&searcher, terms, contains)? else {
            return Ok(None);
        };

        let count = match (terms, contains) {
            ([], []) => searcher.num_docs() as usize,
            _ => query
                .count(&searcher)
                .map_err(|e| anyhow!("tantivy count: {e}"))?,
        };

        Ok(Some(count))
    }

    /// 该字段在本归档里是否按整值（`raw` 分词器）建索引。
    pub fn indexes_whole_values(&self, name: &str) -> bool {
        self.whole_value_field(name).is_some()
    }

    fn whole_value_field(&self, name: &str) -> Option<Field> {
        let field = self.schema.get_field(name).ok()?;
        indexes_whole_values(self.schema.get_field_entry(field)).then_some(field)
    }

    /// 每个字段都取 `sets` 里它的值之一的文档；没有条件时是全部文档。某个字段不是整值索引时为
    /// `None`。
    fn whole_value_query(&self, sets: &[(String, Vec<String>)]) -> Option<Box<dyn Query>> {
        let mut queries = Vec::with_capacity(sets.len());

        for (name, values) in sets {
            let field = self.whole_value_field(name)?;
            let mut alternatives = values
                .iter()
                .map(|value| term_query(field, value))
                .collect::<Vec<_>>();

            queries.push(match alternatives.len() {
                1 => alternatives.pop().expect("one value"),
                _ => Box::new(BooleanQuery::union(alternatives)),
            });
        }

        Some(conjunction(queries))
    }

    /// `terms` 全部成立、`contains` 的子串都出现的文档；都没有时是全部文档。某个等值条件的
    /// 字段不是整值索引、某个子串数不出来时为 `None`。
    fn matching_query(
        &self,
        searcher: &Searcher,
        terms: &[(String, String)],
        contains: &[(String, String)],
    ) -> Result<Option<Box<dyn Query>>> {
        let mut queries = Vec::with_capacity(terms.len() + contains.len());

        for (name, value) in terms {
            let Some(field) = self.whole_value_field(name) else {
                return Ok(None);
            };

            queries.push(term_query(field, value));
        }

        for (name, term) in contains {
            let Some(query) = self.contains_query(searcher, name, term)? else {
                return Ok(None);
            };

            queries.push(query);
        }

        Ok(Some(conjunction(queries)))
    }

    /// 每次调用新开 reader，见 [`Self::count_term`]。
    fn searcher(&self) -> Result<Searcher> {
        let reader: tantivy::IndexReader = self
            .index
            .reader_builder()
            .reload_policy(ReloadPolicy::Manual)
            .try_into()
            .map_err(|e| anyhow!("tantivy reader: {e}"))?;

        Ok(reader.searcher())
    }
}

/// 全部成立的文档；没有条件时是全部文档。
fn conjunction(mut queries: Vec<Box<dyn Query>>) -> Box<dyn Query> {
    match queries.len() {
        0 => Box::new(AllQuery),
        1 => queries.pop().expect("one query"),
        _ => Box::new(BooleanQuery::intersection(queries)),
    }
}

fn term_query(field: Field, value: &str) -> Box<dyn Query> {
    Box::new(TermQuery::new(
        Term::from_field_text(field, value),
        IndexRecordOption::Basic,
    ))
}

/// 分词字段：按 [`TEXT_TOKENIZER`] 分词，只记文档。裁剪与计数只问哪些文档有某个词，用不到词频
/// 与位置。
fn text_options() -> TextOptions {
    TextOptions::default().set_indexing_options(
        TextFieldIndexing::default()
            .set_tokenizer(TEXT_TOKENIZER)
            .set_index_option(IndexRecordOption::Basic),
    )
}

/// 一行 JSON 字段第一层的每个键：「有这个键」与「这个键的值是什么」各一个词，值按 `json_text`
/// 的规则写成文本。不是 JSON 对象的值（被遮掩的值也是）没有词，与 `json_text` 给出 NULL 一致。
/// 过长的词 Tantivy 会丢掉；查询不拿这样长的字面量裁剪，见
/// [`crate::infra::query::tantivy_pruner::indexed_atoms`]。
fn add_members(doc: &mut TantivyDocument, field: Field, document: &str) {
    let Some(object) = json_members::object_of(document) else {
        return;
    };

    for (key, value) in object {
        let Some(text) = json_members::member_text(value) else {
            continue;
        };

        doc.add_text(field, key_term(&key));
        doc.add_text(field, key_value_term(&key, &text));
    }
}

/// 文本字段按整值（`STRING`，`raw` 分词器）建索引：一个值就是一个 term。
fn indexes_whole_values(entry: &FieldEntry) -> bool {
    match entry.field_type() {
        TantivyFieldType::Str(options) => options
            .get_indexing_options()
            .is_some_and(|indexing| indexing.tokenizer() == "raw"),
        _ => false,
    }
}

pub struct TantivyArchiveOpener;

impl TantivyArchiveOpener {
    /// 从 object_store 读 footer 后构造 IndexHandle。之后 tantivy 的读先查归档自带的 hot
    /// cache，未命中才转成对单 blob 的 sub-range `get_range`，**不下载整 archive**。
    pub async fn open_from_object_store(
        store: Arc<dyn ObjectStore>,
        location: ObjectPath,
        size: u64,
    ) -> Result<IndexHandle> {
        let puffin_dir = PuffinDirReader::from_object_store(store, location, size).await?;
        Self::index_from_dir(puffin_dir)
    }

    /// 不知道对象大小时打开：一次后缀范围读取拿到末尾与对象大小，不发 HEAD。对象不存在时
    /// 错误链里是 [`object_store::Error::NotFound`]。
    pub async fn open_tail(
        store: Arc<dyn ObjectStore>,
        location: ObjectPath,
    ) -> Result<IndexHandle> {
        let puffin_dir = PuffinDirReader::from_object_store_tail(store, location).await?;
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
        index
            .tokenizers()
            .register(TEXT_TOKENIZER, text::text_analyzer());

        let schema = index.schema();

        Ok(IndexHandle {
            index,
            schema,
            puffin_dir,
        })
    }
}

// =====================================================================
//  Footer cache value（puffin meta + payload + schema；不含 archive 全字节）
// =====================================================================

pub use crate::tantivy::TantivyFooter;

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

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
            stream_type: crate::domain::stream::StreamType::LOGS,
            schema: DomainSchema {
                fields: vec![
                    FieldDef {
                        name: "level".into(),
                        data_type: FieldType::Utf8,
                        nullable: false,
                        index_type: None,
                        indexed: false,
                        encrypted: false,
                        exact: false,
                    },
                    FieldDef {
                        name: "message".into(),
                        data_type: FieldType::Utf8,
                        nullable: false,
                        index_type: None,
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
            b.add_doc(None, &v).unwrap();
        }
        let bytes = b.commit_and_archive().unwrap();
        let store: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
        let path = ObjectPath::from("exact.ttv");
        store.put(&path, bytes.clone().into()).await.expect("put");
        let handle = TantivyArchiveOpener::open_from_object_store(store, path, bytes.len() as u64)
            .await
            .expect("open");

        // 整值精确匹配命中（含大写和标点，TEXT 分词下会 miss）。
        assert_eq!(
            handle.count_term("message", "my-service/API").unwrap(),
            Some(1)
        );
        // 子串 / 分词后的片段查不到（STRING 不切分 → 等值语义）。
        assert_eq!(handle.count_term("message", "my").unwrap(), Some(0));
        assert_eq!(handle.count_term("message", "api").unwrap(), Some(0));
    }

    /// 加密字段即便配置倒排索引也不进 Tantivy schema（列里是密文，索引它只会误裁）。
    #[test]
    fn encrypted_indexed_field_is_skipped() {
        let mut s = stream_with_indexed_message();
        s.schema.fields[1].encrypted = true;
        // 只剩加密字段可选 → 无可索引字段 → None。
        assert!(TantivyArchiveBuilder::try_new(&s).unwrap().is_none());
    }

    #[test]
    fn bloom_and_skip_fields_do_not_fall_through_to_text_indexes() {
        for index_type in [StreamIndexType::Bloom, StreamIndexType::Skip] {
            let mut stream = stream_with_indexed_message();
            stream.schema.fields[1].configure_index(true, index_type);
            assert!(
                TantivyArchiveBuilder::try_new(&stream).unwrap().is_none(),
                "{index_type:?} must not create a Tantivy field"
            );
        }
    }

    /// 存量 json 全文索引不受影响：builder 仍为兼容推导出的 Json full_text 字段建 TEXT 索引
    /// （spec stream-index-config「存量全文索引兼容」——新配置已由 API 层 400 拦下，写侧
    /// 保持 `Utf8 | Json` 原状，json full_text 检索与裁剪行为不变）。
    #[tokio::test]
    async fn json_full_text_field_still_builds_text_index() {
        use object_store::{ObjectStoreExt, memory::InMemory, path::Path as ObjectPath};

        let mut s = stream_with_indexed_message();
        s.schema.fields[1].data_type = FieldType::Json;
        let mut b = TantivyArchiveBuilder::try_new(&s)
            .unwrap()
            .expect("json full_text 仍应建 TEXT 索引");
        let mut v = HashMap::new();
        v.insert("message", r#"{"error": "panic at line 1"}"#);
        b.add_doc(None, &v).unwrap();
        let bytes = b.commit_and_archive().unwrap();

        let store: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
        let path = ObjectPath::from("json.ttv");
        store.put(&path, bytes.clone().into()).await.expect("put");
        let size = bytes.len() as u64;
        let handle = TantivyArchiveOpener::open_from_object_store(store, path, size)
            .await
            .expect("open");
        assert_eq!(handle.may_contain("message", "panic").unwrap(), Some(true));
        assert_eq!(
            handle.may_contain("message", "banana").unwrap(),
            Some(false)
        );
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
            b.add_doc(None, &v).unwrap();
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
        let contains = |term: &str| {
            handle
                .count_exact(&[], &[("message".to_owned(), term.to_owned())])
                .unwrap()
        };
        assert_eq!(contains("panic"), Some(2));
        assert_eq!(contains("banana"), Some(0));
        // 未索引字段答不出（field 不在 tantivy schema）
        assert_eq!(handle.count_term("level", "info").unwrap(), None);
    }

    /// 分词字段入索引时过了分词器（小写化 + 按非字母数字切分），拿原值查 term 会查出 0：
    /// 含标点或大写的值都是这样。`TantivyPruner` 拿这个 0 去裁文件就会漏数据——字段从全文改成
    /// 精确以后，改之前写下的归档里它仍是分词字段。所以 `count_term` 只回答整值字段，分词字段
    /// 答 `None`；`MATCH` 的词用 [`IndexHandle::may_contain`] 按子串判断。
    #[tokio::test]
    async fn count_term_answers_only_for_whole_value_fields() {
        use object_store::{ObjectStoreExt, memory::InMemory, path::Path as ObjectPath};
        let s = stream_with_indexed_message();
        let mut b = TantivyArchiveBuilder::try_new(&s).unwrap().unwrap();
        let mut v = HashMap::new();
        v.insert("message", "Service my-api FAILED");
        v.insert("level", "info");
        b.add_doc(None, &v).unwrap();
        let bytes = b.commit_and_archive().unwrap();
        let store: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
        let path = ObjectPath::from("tok.ttv");
        store.put(&path, bytes.clone().into()).await.expect("put");
        let handle = TantivyArchiveOpener::open_from_object_store(store, path, bytes.len() as u64)
            .await
            .expect("open");

        // 原样的大写 / 带标点的值、碰巧是一个词的值，分词字段都不按整值回答。
        for value in ["FAILED", "my-api", "failed", "Service my-api FAILED"] {
            assert_eq!(
                handle.count_term("message", value).unwrap(),
                None,
                "{value}"
            );
        }

        // 子串仍按分词索引判断。
        assert_eq!(handle.may_contain("message", "my-api").unwrap(), Some(true));
        assert_eq!(handle.may_contain("message", "panic").unwrap(), Some(false));
    }

    /// 整值字段的等值命中数就是行数；多个条件取交集；不带条件时是全部文档数；分词字段的
    /// 命中数不等于等值过滤的行数，不给出计数。
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn count_exact_counts_rows_only_for_whole_value_fields() {
        use object_store::{ObjectStoreExt, memory::InMemory, path::Path as ObjectPath};

        let mut stream = stream_with_indexed_message();
        stream.schema.fields[0].indexed = true;
        stream.schema.fields[0].exact = true;
        let mut builder = TantivyArchiveBuilder::try_new(&stream).unwrap().unwrap();

        for (level, message) in [
            ("error", "disk full"),
            ("error", "disk full"),
            ("warn", "disk full"),
            ("error", "timeout"),
        ] {
            let mut values = HashMap::new();
            values.insert("level", level);
            values.insert("message", message);
            builder.add_doc(None, &values).unwrap();
        }

        let bytes = builder.commit_and_archive().unwrap();
        let store: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
        let path = ObjectPath::from("count.ttv");
        store.put(&path, bytes.clone().into()).await.expect("put");
        let handle = TantivyArchiveOpener::open_from_object_store(store, path, bytes.len() as u64)
            .await
            .expect("open");

        let terms = |pairs: &[(&str, &str)]| -> Vec<(String, String)> {
            pairs
                .iter()
                .map(|(field, value)| ((*field).to_owned(), (*value).to_owned()))
                .collect()
        };

        let counts = tokio::task::spawn_blocking(move || {
            (
                handle
                    .count_exact(&terms(&[("level", "error")]), &[])
                    .unwrap(),
                handle
                    .count_exact(&terms(&[("level", "error"), ("level", "warn")]), &[])
                    .unwrap(),
                handle.count_exact(&[], &[]).unwrap(),
                handle
                    .count_exact(&terms(&[("message", "timeout")]), &[])
                    .unwrap(),
                handle.count_exact(&terms(&[("absent", "x")]), &[]).unwrap(),
            )
        })
        .await
        .unwrap();

        assert_eq!(counts, (Some(3), Some(0), Some(4), None, None));
    }

    /// 子串条件数的是值里出现这个子串的行，与 `ILIKE '%term%'` 一样：落在更长的词里也算。长词
    /// 里可能有它、它不全是小写字母数字、字段不是分词索引时不给出计数。
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn count_exact_counts_substrings_the_way_ilike_matches_them() {
        use object_store::{ObjectStoreExt, memory::InMemory, path::Path as ObjectPath};

        let mut stream = stream_with_indexed_message();
        stream.schema.fields[0].indexed = true;
        stream.schema.fields[0].exact = true;
        let mut builder = TantivyArchiveBuilder::try_new(&stream).unwrap().unwrap();

        for (level, message) in [
            ("error", "Panic in worker"),
            ("error", "worker panicked"),
            ("warn", "all good"),
            (
                "warn",
                "sha256=9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08",
            ),
        ] {
            let mut values = HashMap::new();
            values.insert("level", level);
            values.insert("message", message);
            builder.add_doc(None, &values).unwrap();
        }

        let bytes = builder.commit_and_archive().unwrap();
        let store: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
        let path = ObjectPath::from("contains.ttv");
        store.put(&path, bytes.clone().into()).await.expect("put");
        let handle = TantivyArchiveOpener::open_from_object_store(store, path, bytes.len() as u64)
            .await
            .expect("open");

        let pairs = |pairs: &[(&str, &str)]| -> Vec<(String, String)> {
            pairs
                .iter()
                .map(|(field, value)| ((*field).to_owned(), (*value).to_owned()))
                .collect()
        };

        let counts = tokio::task::spawn_blocking(move || {
            let count = |terms: &[(&str, &str)], contains: &[(&str, &str)]| {
                handle.count_exact(&pairs(terms), &pairs(contains)).unwrap()
            };

            [
                count(&[], &[("message", "panic")]),
                count(&[("level", "error")], &[("message", "work")]),
                count(&[("level", "warn")], &[("message", "panic")]),
                count(&[], &[("message", "zzz")]),
                count(&[], &[("message", "sha256")]),
                // Inside the long digest, which is not in the dictionary.
                count(&[], &[("message", "d081")]),
                count(&[], &[("message", "Panic")]),
                count(&[], &[("level", "error")]),
            ]
        })
        .await
        .unwrap();

        assert_eq!(
            counts,
            [
                Some(2),
                Some(2),
                Some(0),
                Some(0),
                Some(1),
                None,
                None,
                None
            ]
        );
    }

    /// 柱子按行时间分：窗外的行、没有行时间的行不计入；错误按 level 转大写是否等于 `ERROR`
    /// 另计；过滤字段或错误字段不是整值索引时不给出计数。
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn count_bars_splits_matching_rows_by_time_and_level() {
        use object_store::{ObjectStoreExt, memory::InMemory, path::Path as ObjectPath};

        let mut stream = stream_with_indexed_message();
        stream.schema.fields[0].indexed = true;
        stream.schema.fields[0].exact = true;
        let mut builder = TantivyArchiveBuilder::try_new(&stream).unwrap().unwrap();

        for (time, level, message) in [
            (Some(5), "ERROR", "disk full"),
            (Some(15), "error", "disk full"),
            (Some(15), "Error", "timeout"),
            (Some(19), "warn", "disk full"),
            (Some(25), "error", "disk full"),
            (Some(40), "error", "disk full"),
            (None, "error", "disk full"),
        ] {
            let mut values = HashMap::new();
            values.insert("level", level);
            values.insert("message", message);
            builder.add_doc(time, &values).unwrap();
        }

        let bytes = builder.commit_and_archive().unwrap();
        let store: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
        let path = ObjectPath::from("bars.ttv");
        store.put(&path, bytes.clone().into()).await.expect("put");
        let handle = TantivyArchiveOpener::open_from_object_store(store, path, bytes.len() as u64)
            .await
            .expect("open");

        let query = |terms: &[(&str, &str)], errors: Option<&str>| BarQuery {
            terms: terms
                .iter()
                .map(|(field, value)| ((*field).to_owned(), (*value).to_owned()))
                .collect(),
            contains: Vec::new(),
            // Bars of ten from 0; the row at 40 lies past the window.
            window: 5..30,
            origin: 0,
            width: 10,
            errors: errors.map(str::to_owned),
        };

        let tally = |total, errors| BarTally { total, errors };

        let counted = tokio::task::spawn_blocking(move || {
            (
                handle.count_bars(&query(&[], Some("level"))).unwrap(),
                handle
                    .count_bars(&query(&[("level", "error")], None))
                    .unwrap(),
                handle
                    .count_bars(&query(&[("message", "timeout")], None))
                    .unwrap(),
                handle.count_bars(&query(&[], Some("message"))).unwrap(),
            )
        })
        .await
        .unwrap();

        assert_eq!(
            counted.0,
            Some(BTreeMap::from([
                (0, tally(1, 1)),
                (1, tally(3, 2)),
                (2, tally(1, 1)),
            ]))
        );
        assert_eq!(
            counted.1,
            Some(BTreeMap::from([(1, tally(1, 0)), (2, tally(1, 0))]))
        );
        assert_eq!((counted.2, counted.3), (None, None));
    }

    /// 一行：行时间与各字段的值。
    type Row<'a> = (Option<i64>, Vec<(&'a str, &'a str)>);

    /// 按行序把 `rows` 建成一个归档并打开。
    async fn open_archive(stream: &StreamDefinition, rows: &[Row<'_>]) -> IndexHandle {
        use object_store::{ObjectStoreExt, memory::InMemory, path::Path as ObjectPath};

        let mut builder = TantivyArchiveBuilder::try_new(stream).unwrap().unwrap();

        for (time, values) in rows {
            builder
                .add_doc(*time, &values.iter().copied().collect())
                .unwrap();
        }

        let bytes = builder.commit_and_archive().unwrap();
        let store: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
        let path = ObjectPath::from("archive.ttv");
        store.put(&path, bytes.clone().into()).await.expect("put");

        TantivyArchiveOpener::open_from_object_store(store, path, bytes.len() as u64)
            .await
            .expect("open")
    }

    fn stream_with_exact_level() -> StreamDefinition {
        let mut stream = stream_with_indexed_message();
        stream.schema.fields[0].indexed = true;
        stream.schema.fields[0].exact = true;
        stream
    }

    fn sets(pairs: &[(&str, &[&str])]) -> Vec<(String, Vec<String>)> {
        pairs
            .iter()
            .map(|(field, values)| {
                (
                    (*field).to_owned(),
                    values.iter().map(|value| (*value).to_owned()).collect(),
                )
            })
            .collect()
    }

    /// 命中的文档换成它们在文件里的行号，不论写入时落在哪个段；命中太多、没有条件或条件落在
    /// 分词字段上时不给出。
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn matching_rows_gives_the_file_rows_of_the_matches() {
        let rows = (0..300)
            .map(|row| {
                let level = if row % 7 == 0 { "error" } else { "info" };
                (None, vec![("level", level), ("message", "disk full")])
            })
            .collect::<Vec<_>>();

        let handle = open_archive(&stream_with_exact_level(), &rows).await;

        let found = tokio::task::spawn_blocking(move || {
            (
                handle
                    .matching_rows(&sets(&[("level", &["error"])]), 100)
                    .unwrap(),
                handle
                    .matching_rows(&sets(&[("level", &["absent"])]), 100)
                    .unwrap(),
                handle
                    .matching_rows(&sets(&[("level", &["info"])]), 100)
                    .unwrap(),
                handle.matching_rows(&[], 100).unwrap(),
                handle
                    .matching_rows(&sets(&[("message", &["disk"])]), 100)
                    .unwrap(),
                handle
                    .matching_rows(&sets(&[("level", &["absent", "error"])]), 100)
                    .unwrap(),
            )
        })
        .await
        .unwrap();

        let errors = (0..300).step_by(7).collect::<Vec<u64>>();

        assert_eq!(found.0, Some(errors.clone()));
        assert_eq!(found.1, Some(Vec::new()));
        assert_eq!((found.2, found.3, found.4), (None, None, None));
        assert_eq!(
            found.5,
            Some(errors),
            "an IN list matches any of its values"
        );
    }

    /// 每个整值数它在多少行里出现，只读词典：空串不计；字段不是整值索引、某段的值太多时不给出。
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn count_values_counts_the_rows_of_each_whole_value() {
        let rows = ["error", "error", "warn", "", "info", "debug", "error"]
            .into_iter()
            .map(|level| (None, vec![("level", level), ("message", "disk full")]))
            .collect::<Vec<_>>();

        let handle = open_archive(&stream_with_exact_level(), &rows).await;

        let counted = tokio::task::spawn_blocking(move || {
            (
                handle.count_values("level", 100).unwrap(),
                // Four values over at most three segments: one segment holds two of them.
                handle.count_values("level", 1).unwrap(),
                handle.count_values("message", 100).unwrap(),
            )
        })
        .await
        .unwrap();

        let expected = [("error", 3), ("warn", 1), ("info", 1), ("debug", 1)]
            .into_iter()
            .map(|(value, count)| (value.to_owned(), count))
            .collect::<HashMap<_, _>>();

        assert_eq!(counted.0, Some(expected));
        assert_eq!((counted.1, counted.2), (None, None));
    }

    /// 一个长到没能建索引的值让这个字段的按值计数不再给出；行号与行数照常。
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn an_overlong_value_withholds_the_value_counts_of_its_field() {
        let long = "x".repeat(MAX_TOKEN_LEN + 1);

        let rows = vec![
            (
                None,
                vec![("level", long.as_str()), ("message", "disk full")],
            ),
            (None, vec![("level", "error"), ("message", "disk full")]),
        ];

        let handle = open_archive(&stream_with_exact_level(), &rows).await;

        let answers = tokio::task::spawn_blocking(move || {
            (
                handle.count_values("level", 100).unwrap(),
                handle
                    .matching_rows(&sets(&[("level", &["error"])]), 100)
                    .unwrap(),
                handle.count_exact(&[], &[]).unwrap(),
            )
        })
        .await
        .unwrap();

        assert_eq!(answers, (None, Some(vec![1]), Some(2)));
    }

    /// 子串判断：词典里有包含它的词、或长词的三元组里有它时可能有，都没有时一定没有。带标点的
    /// 子串按段判断，有一段一定没有就一定没有；整值字段、没有能查的段时答不出。
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn may_contain_looks_into_tokens_and_long_tokens() {
        const DIGEST: &str = "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08";

        let line = format!("worker panicked sha256={DIGEST}");
        let rows = vec![(None, vec![("message", line.as_str()), ("level", "error")])];
        let handle = open_archive(&stream_with_exact_level(), &rows).await;

        let answers = tokio::task::spawn_blocking(move || {
            let answer = |field: &str, term: &str| handle.may_contain(field, term).unwrap();

            vec![
                answer("message", "panic"),
                answer("message", "panics"),
                answer("message", "d081884c"),
                answer("message", "cafe"),
                answer("message", "PANIC"),
                answer("message", "sha256=d081"),
                answer("message", "worker-cafe"),
                answer("message", "---"),
                answer("level", "err"),
            ]
        })
        .await
        .unwrap();

        assert_eq!(
            answers,
            vec![
                Some(true),
                Some(false),
                Some(true),
                Some(false),
                Some(true),
                Some(true),
                Some(false),
                None,
                None
            ]
        );
    }

    /// 中文按二元组判断：子串的二元组都在词典里才可能有，单个字落在某个二元组里也算。
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn may_contain_answers_cjk_substrings() {
        let rows = vec![(
            None,
            vec![("message", "用户登录失败，订单order=7"), ("level", "error")],
        )];
        let handle = open_archive(&stream_with_exact_level(), &rows).await;

        let answers = tokio::task::spawn_blocking(move || {
            let answer = |term: &str| handle.may_contain("message", term).unwrap();

            vec![
                answer("登录"),
                answer("登录失败"),
                answer("失败登录"),
                answer("录"),
                answer("猫"),
                answer("订单order"),
                answer("登录 cafe"),
            ]
        })
        .await
        .unwrap();

        assert_eq!(
            answers,
            vec![
                Some(true),
                Some(true),
                Some(false),
                Some(true),
                Some(false),
                Some(true),
                Some(false)
            ]
        );
    }
}
