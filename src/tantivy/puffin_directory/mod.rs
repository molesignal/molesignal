// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 把 tantivy 的 `Directory` 接口适配到 Puffin v1 文件。
//!
//! 写路径：[`PuffinDirWriter`] 内部走 tempdir + MmapDirectory，tantivy 正常 IO；
//! `to_puffin_bytes()` 把每个产出的 tantivy 文件作为一个 Puffin blob 拼接。
//!
//! 读路径：[`PuffinDirReader`] 持有 `PuffinBytesReader`（即 `ObjectStore` + `Path` + `size`），
//! tantivy 每次 `get_file_handle` → `PuffinSliceHandle::read_bytes_async` →
//! sub-range `get_range`。所有写方法 panic（read-only）。

pub mod empty_directory;
pub mod reader;
pub mod writer;

use std::{path::PathBuf, sync::Arc};

use bytes::Bytes;
pub use empty_directory::build_footer_cache_for_test_only;
use hashbrown::HashMap;
pub use reader::PuffinDirReader;
use tantivy::schema::Schema;
pub use writer::PuffinDirWriter;

use crate::tantivy::puffin::PuffinMeta;

/// 允许进 puffin 的 tantivy 文件扩展名。
/// 沿用既有约定：`{term,idx,pos,store,fast,fieldnorm,del,json,lock}`，
/// 外加我们自己写入的 footer-cache blob 走的虚拟文件名常量 [`FOOTER_CACHE_BLOB_TAG`]。
pub const ALLOWED_FILE_EXT: &[&str] = &[
    "term",
    "idx",
    "pos",
    "store",
    "fast",
    "fieldnorm",
    "del",
    "json",
    "lock",
];

/// `tantivy::meta.json` 的固定文件名。
pub const META_JSON: &str = "meta.json";

/// 我们自己写入 puffin 的 segment-meta cache blob 的 `blob_tag`（不是真实 tantivy 文件）。
/// reader 端识别后从 footer cache 复用而非作为虚拟文件返。
pub const FOOTER_CACHE_BLOB_TAG: &str = "__o2_footer_cache__";

/// Tantivy 归档的 cache value：包含解析后的 [`PuffinMeta`] + 原始 footer payload bytes
/// + 解析后的 tantivy schema + sync atomic_read 目标文件预物化字节（通常含
///   `meta.json`，~几 KB）。**不包含** archive 整体 bytes —— 这是与旧 footer cache
///   的核心区别。`size_bytes` 用作 cache eviction 估算。
///
/// `atomic_files` 设计：tantivy 0.25 的 `Index::open(Directory)` 走 sync
/// `Directory::atomic_read` 读 `meta.json`；reader 是 read-only 不能发 async IO
/// 兜底，所以构造期就 async 预物化 `meta.json` 等 atomic_read 目标到此 map。
/// Cache 命中路径（`PuffinDirReader::from_cached_meta`）复用同一份字节，避免
/// 任何 IO。
#[derive(Clone, Debug)]
pub struct TantivyFooter {
    pub puffin_meta: Arc<PuffinMeta>,
    pub footer_payload_bytes: Bytes,
    pub schema: Schema,
    /// `path → bytes`：sync atomic_read 命中走 map，miss 返 FileDoesNotExist。
    pub atomic_files: Arc<HashMap<PathBuf, Bytes>>,
    /// 归档对象的总字节数。footer cache 命中重开 `IndexHandle` 时复用，省掉
    /// 一次 `object_store.head` —— 这正是 footer cache 短路 object_store 的关键。
    pub object_size: u64,
}

/// 已知 tantivy sync `atomic_read` 触碰的 puffin blob_tag。构造 reader 时按这个
/// 列表把 blob 内容一次性预下载到 `atomic_files`。
pub const SYNC_ATOMIC_READ_TARGETS: &[&str] = &[META_JSON];

impl TantivyFooter {
    pub fn size_bytes(&self) -> usize {
        let meta_size = serde_json::to_vec(&*self.puffin_meta)
            .map(|v| v.len())
            .unwrap_or(0);
        let atomic_size: usize = self.atomic_files.values().map(|b| b.len()).sum();
        meta_size + self.footer_payload_bytes.len() + atomic_size + 1024
    }
}
