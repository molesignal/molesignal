// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Profile blob layout, compression, IO and retention.

use std::sync::Arc;

use bytes::Bytes;
use kernel::{Error, Result, ids::Id};
use object_store::{ObjectStore, ObjectStoreExt, path::Path as ObjPath};

const PROFILE_ARCHIVE_LAYOUT_VERSION: &str = "v1";

/// Archive object key:
/// `blobs/v1/<org_id>/profiles/<service>/<profile_type>/<yyyymmdd>/<profile_id>.pprof.zst`.
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

/// Folds non-path-safe characters into `_`; an empty string falls back to `unknown`.
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

/// Micros since epoch -> `YYYYMMDD` (civil_from_days, Howard Hinnant's algorithm, no
/// dependencies).
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

/// zstd compression (level 3, the same as RUM replay archiving).
pub fn zstd_compress(bytes: &[u8]) -> Result<Vec<u8>> {
    zstd::stream::encode_all(bytes, 3).map_err(|e| Error::internal(format!("profile zstd: {e}")))
}

/// zstd decompression.
pub fn zstd_decompress(bytes: &[u8]) -> Result<Vec<u8>> {
    zstd::stream::decode_all(bytes).map_err(|e| Error::internal(format!("profile unzstd: {e}")))
}

/// Archives a raw pprof protobuf to the object store with zstd and returns the archived
/// byte count.
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

/// Reads back an archive object and decompresses it with zstd into a raw pprof protobuf
/// (for aggregation / download).
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

/// Extracts the `YYYYMMDD` date segment from a v1 archive key; returns `None` for a
/// non-archive or malformed key.
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

/// Deletes the archive blobs under an org's `blobs/v1/<org>/profiles/` prefix whose date is
/// earlier than the retention cutoff.
///
/// The main profiles Artifact follows the stream retention and is cleaned up automatically
/// by FileCatalog tombstones plus GC. An archive blob is a side object in the object store
/// and is not in the FileCatalog, so the retention sweep must call this function to clean
/// it up too (storage spec: on expiry, clean up both the parquet metadata and the archive
/// blobs). Archives are bucketed by `yyyymmdd`, so comparing at day granularity is enough.
/// Best-effort: a failed individual delete is only skipped, and the next sweep is the
/// backstop; returns the number of blobs successfully deleted.
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
