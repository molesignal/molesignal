// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 当 reader 端 tantivy 请求一个 puffin 里不存在的 segment 文件（例如某 segment
//! 没启用 fieldnorm 但 tantivy 仍尝试 open `.fieldnorm`）时，从一份「空 puffin 目录」
//! 的对应扩展名返回安全字节。
//!
//! 实现策略：runtime lazy 初始化（OnceLock）。第一次调用时在 tempdir 创建一个空 tantivy
//! 索引 → 收集所有 segment 文件 → bytes 入 map。
//! 优点：不需要 build script、不需要 include_bytes、tantivy 版本升级时自动跟随。
//! 代价：首次访问支付一次 ~ms 级初始化（包在 OnceLock 内，仅一次）。

use std::{collections::HashMap, fs, sync::OnceLock};

use bytes::Bytes;
use tantivy::{
    Index,
    directory::MmapDirectory,
    schema::{STRING, Schema},
};

/// `(extension → bytes)`：每个 tantivy 段文件作为一份兜底字节。
/// 同扩展名的多份文件不可能出现（tantivy 一个段一个文件）；取首个匹配的。
fn build() -> HashMap<String, Bytes> {
    let tmp = tempfile::tempdir().expect("empty puffin dir tempdir");
    // tantivy 0.25 不允许 0 字段的 schema → 加一个 STRING 字段触发完整 segment 产生。
    let mut sb = Schema::builder();
    let _ = sb.add_text_field("__placeholder", STRING);
    let schema = sb.build();
    let mmap = MmapDirectory::open(tmp.path()).expect("open mmap");
    let index =
        Index::create(mmap, schema, tantivy::IndexSettings::default()).expect("create empty index");
    let mut writer: tantivy::IndexWriter = index.writer(50_000_000).expect("writer");
    // 强制写出一个 commit（即便没文档）让段文件落盘。
    writer.commit().expect("commit");
    drop(writer);
    drop(index);
    let mut map = HashMap::new();
    for entry in fs::read_dir(tmp.path()).expect("read tempdir") {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();
        let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
            continue;
        };
        if let Ok(bytes) = fs::read(&path) {
            map.entry(ext.to_string())
                .or_insert_with(|| Bytes::from(bytes));
        }
    }
    map
}

/// 返回扩展名对应的"空 tantivy 文件"字节；扩展名不在内部 map 时返 None。
pub fn get_empty_file_bytes(ext: &str) -> Option<Bytes> {
    static MAP: OnceLock<HashMap<String, Bytes>> = OnceLock::new();
    MAP.get_or_init(build).get(ext).cloned()
}

/// 仅用于 PuffinDirWriter 的测试：构造 tantivy footer cache 占位。
/// 现阶段不在生产路径上使用；保留接口供未来真正 bake segment meta 用。
pub fn build_footer_cache_for_test_only() -> Bytes {
    Bytes::from_static(b"__placeholder_footer_cache__")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_directory_yields_meta_json_at_minimum() {
        // tantivy commit 至少会产 meta.json + segment 描述文件。
        let json = get_empty_file_bytes("json");
        assert!(json.is_some());
    }
}
