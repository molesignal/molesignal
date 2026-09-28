// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Profile blob layout, compression, IO and retention.

use std::sync::Arc;

use bytes::Bytes;
use object_store::{ObjectStore, ObjectStoreExt, path::Path as ObjPath};

use crate::shared::{Error, Result, ids::Id};

const PROFILE_ARCHIVE_LAYOUT_VERSION: &str = "v1";

/// 归档对象 key：
/// `blobs/v1/<org_id>/profiles/<service>/<profile_type>/<yyyymmdd>/<profile_id>.pprof.zst`。
pub fn archive_object_key(
    org_id: &Id,
    service: &str,
    profile_type: &str,
    start_time_micros: i64,
    profile_id: &Id,
) -> String {
    format!(
        "blobs/{PROFILE_ARCHIVE_LAYOUT_VERSION}/{}/profiles/{}/{}/{}/{}.pprof.zst",
        org_id.0,
        sanitize_key_segment(service),
        sanitize_key_segment(profile_type),
        yyyymmdd(start_time_micros),
        profile_id.0,
    )
}

/// 把非路径安全字符折叠为 `_`，空串回退 `unknown`。
fn sanitize_key_segment(s: &str) -> String {
    let out: String = s
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if out.is_empty() {
        "unknown".to_string()
    } else {
        out
    }
}

/// micros since epoch → `YYYYMMDD`（civil_from_days，Howard Hinnant 算法，无依赖）。
fn yyyymmdd(micros: i64) -> String {
    let days = micros.div_euclid(86_400_000_000);
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}{m:02}{d:02}")
}

/// zstd 压缩（level 3，与 RUM replay 归档一致）。
pub fn zstd_compress(bytes: &[u8]) -> Result<Vec<u8>> {
    zstd::stream::encode_all(bytes, 3).map_err(|e| Error::internal(format!("profile zstd: {e}")))
}

/// zstd 解压。
pub fn zstd_decompress(bytes: &[u8]) -> Result<Vec<u8>> {
    zstd::stream::decode_all(bytes).map_err(|e| Error::internal(format!("profile unzstd: {e}")))
}

/// 把裸 pprof protobuf zstd 归档到 object store，返回归档字节数。
pub async fn put_archive(
    object_store: &Arc<dyn ObjectStore>,
    key: &str,
    raw_pprof: &[u8],
) -> Result<u64> {
    let compressed = zstd_compress(raw_pprof)?;
    let bytes = compressed.len() as u64;
    let path =
        ObjPath::parse(key).map_err(|e| Error::internal(format!("profile object path: {e}")))?;
    object_store
        .put(&path, Bytes::from(compressed).into())
        .await
        .map_err(|e| Error::internal(format!("profile archive put: {e}")))?;
    Ok(bytes)
}

/// 读回归档对象并 zstd 解压为裸 pprof protobuf（聚合 / 下载用）。
pub async fn get_archive(object_store: &Arc<dyn ObjectStore>, key: &str) -> Result<Vec<u8>> {
    let path =
        ObjPath::parse(key).map_err(|e| Error::internal(format!("profile object path: {e}")))?;
    let got = object_store
        .get(&path)
        .await
        .map_err(|e| Error::internal(format!("profile archive get: {e}")))?;
    let bytes = got
        .bytes()
        .await
        .map_err(|e| Error::internal(format!("profile archive read: {e}")))?;
    zstd_decompress(&bytes)
}

/// 从 v1 归档 key 提取 `YYYYMMDD` 日期段；非归档 / 异常 key 返回 `None`。
pub fn archive_key_date(key: &str) -> Option<&str> {
    let mut segments = key.split('/');
    if segments.next()? != "blobs"
        || segments.next()? != PROFILE_ARCHIVE_LAYOUT_VERSION
        || segments.next()?.is_empty()
        || segments.next()? != "profiles"
        || segments.next()?.is_empty()
        || segments.next()?.is_empty()
    {
        return None;
    }
    let date = segments.next()?;
    let file = segments.next()?;
    if file.is_empty() || segments.next().is_some() {
        return None;
    }
    (date.len() == 8 && date.bytes().all(|byte| byte.is_ascii_digit())).then_some(date)
}

/// 删除某 org `blobs/v1/<org>/profiles/` 前缀下日期早于 retention cutoff 的归档 blob。
///
/// profiles 主 Artifact 随 stream retention 由 FileCatalog tombstone + GC 自动清理；归档 blob 是
/// object store 旁路对象、不在 FileCatalog 内，故 retention sweep 需调用本函数一并清理
/// （storage spec：到期同时清理 parquet 元数据与归档 blob）。归档按 `yyyymmdd` 分桶，
/// 比较到日级即可。best-effort：单个删除失败仅跳过，下一轮 sweep 兜底；返回成功删除数。
pub async fn sweep_expired_archives(
    object_store: &Arc<dyn ObjectStore>,
    org_id: &Id,
    cutoff_micros: i64,
) -> Result<usize> {
    use futures::TryStreamExt;
    let cutoff = yyyymmdd(cutoff_micros);
    let prefix = ObjPath::parse(format!(
        "blobs/{PROFILE_ARCHIVE_LAYOUT_VERSION}/{}/profiles",
        org_id.0
    ))
    .map_err(|e| Error::internal(format!("profile sweep prefix: {e}")))?;
    let mut listing = object_store.list(Some(&prefix));
    let mut deleted = 0usize;
    while let Some(meta) = listing
        .try_next()
        .await
        .map_err(|e| Error::internal(format!("profile sweep list: {e}")))?
    {
        let Some(date) = archive_key_date(meta.location.as_ref()) else {
            continue;
        };
        if date < cutoff.as_str() && object_store.delete(&meta.location).await.is_ok() {
            deleted += 1;
        }
    }
    Ok(deleted)
}

#[cfg(test)]
mod tests;
