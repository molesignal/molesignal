// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Segment 清理（按数量 / 总大小限制）。

use std::path::Path;

use anyhow::Result;

use super::{WAL_SEGMENT_EXT, WAL_SEGMENT_PREFIX};

pub fn evict_old_segments(
    wal_dir: &Path,
    mut segments: Vec<u64>,
    max_segments: Option<usize>,
    max_total_size_bytes: Option<u64>,
    protected_idx: Option<u64>,
) -> Result<()> {
    let is_protected = |idx: u64| protected_idx.is_some_and(|p| p == idx);

    if let Some(max_keep) = max_segments.filter(|v| *v > 0) {
        while segments.len() > max_keep {
            if let Some(&oldest) = segments.first() {
                if is_protected(oldest) {
                    break;
                }
                let old_path =
                    wal_dir.join(format!("{WAL_SEGMENT_PREFIX}{oldest:06}{WAL_SEGMENT_EXT}"));
                let _ = std::fs::remove_file(old_path);
                segments.remove(0);
            } else {
                break;
            }
        }
    }

    if let Some(limit) = max_total_size_bytes.filter(|v| *v > 0) {
        loop {
            let total: u64 = segments
                .iter()
                .map(|idx| {
                    let p = wal_dir.join(format!("{WAL_SEGMENT_PREFIX}{idx:06}{WAL_SEGMENT_EXT}"));
                    std::fs::metadata(p).map(|m| m.len()).unwrap_or(0)
                })
                .sum();
            if total <= limit {
                break;
            }
            if segments.len() <= 1 {
                if protected_idx.is_some() {
                    anyhow::bail!(
                        "WAL total size {} exceeds limit {} and cannot evict more segments",
                        total,
                        limit
                    );
                }
                break;
            }
            let oldest = segments.remove(0);
            if is_protected(oldest) {
                anyhow::bail!("WAL watermark exceeded on active segment");
            }
            let old_path =
                wal_dir.join(format!("{WAL_SEGMENT_PREFIX}{oldest:06}{WAL_SEGMENT_EXT}"));
            let _ = std::fs::remove_file(old_path);
        }
    }
    Ok(())
}
