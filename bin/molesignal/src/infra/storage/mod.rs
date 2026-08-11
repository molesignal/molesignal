// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! parquet + object_store 实现。
//!
//! - [`object`]：基于 `object_store` 抽象的对象存储客户端
//! - [`parquet::writer`]：把 Arrow RecordBatch 序列化为 parquet 并上传
//! - [`parquet::reader`]：从对象存储下载 parquet 并解析为 Arrow
//! - [`compactor`]：周期合并小文件 + retention 扫描

pub mod arrow_schema;
pub mod compactor;
pub mod downsample;
pub mod object;
pub mod parquet;
pub mod parquet_file_meta_dump;
