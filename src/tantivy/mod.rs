// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Puffin-backed Tantivy directory + Puffin v1 binary format.
//!
//! 这个 crate **不依赖 infra**：所有 IO 通过 `object_store::ObjectStore` 注入，
//! 让本 crate 可以独立演进 / 复用。
//!
//! 模块：
//! - [`puffin`]：Puffin v1 文件格式（spec + 二进制 read/write）。
//! - [`puffin_directory`]：把 tantivy 的 `Directory` trait 适配到 Puffin —— 写时
//!   通过 mmap tempdir，序列化时把每个文件作为 blob 拼接；读时 lazy range-read。
//! - [`key_mapping`]：parquet → tantivy puffin sidecar key 映射。

pub mod key_mapping;
pub mod metrics;
pub mod puffin;
pub mod puffin_directory;

pub use puffin::{
    BlobMetadata, BlobTypes, CompressionCodec, FOOTER_SIZE, MAGIC, PuffinFooterFlags, PuffinMeta,
};
pub use puffin_directory::{
    PuffinDirReader, PuffinDirWriter, TantivyFooter, build_footer_cache_for_test_only,
};
