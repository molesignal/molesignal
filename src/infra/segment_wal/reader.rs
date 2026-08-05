// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Segment WAL 读取（mmap + 偏移量解析，支持尾部截断容错）。

use std::{
    fs::{File, OpenOptions},
    path::Path,
};

use anyhow::Result;
use crc32c::crc32c_append;
use lz4_flex::block::decompress_size_prepended;
use memmap2::Mmap;

use super::types::*;

fn decode_payload(flag: u8, payload: &[u8]) -> Result<Vec<u8>> {
    let decoded = if flag & FLAG_LZ4_BIT != 0 {
        decompress_size_prepended(payload)
            .map_err(|e| anyhow::anyhow!("lz4 decompress failed: {e}"))?
    } else {
        payload.to_vec()
    };
    Ok(decoded)
}

/// 读取单个 segment 文件，返回 `(records, had_truncation)`。
///
/// 使用 mmap 把文件映射到内存，基于偏移量顺序解析记录；遇到 magic 不匹配、
/// CRC 校验失败或字节不足时截断到最后一条完好记录。
pub fn read_segment_file<P: AsRef<Path>>(path: P) -> Result<(Vec<WalRecord>, bool)> {
    let path = path.as_ref();
    if !path.exists() {
        return Ok((Vec::new(), false));
    }

    let file = File::open(path)?;
    let file_len = file.metadata()?.len() as usize;
    if file_len == 0 {
        return Ok((Vec::new(), false));
    }

    // SAFETY: segment 文件在读取期间不会被并发写入（WAL writer 持有独占句柄）。
    let mmap = unsafe { Mmap::map(&file)? };
    let data = &mmap[..];

    let mut records = Vec::new();
    let mut offset = 0usize;
    let mut last_good_offset = 0usize;

    while offset < file_len {
        match try_parse_record(data, offset) {
            Ok((record, next_offset)) => {
                records.push(record);
                last_good_offset = next_offset;
                offset = next_offset;
            }
            Err(e) => {
                tracing::warn!(
                    "WAL tail corruption at offset {offset} in {}: {e}, truncating",
                    path.display()
                );
                drop(mmap);
                drop(file);
                let wfile = OpenOptions::new().write(true).open(path)?;
                wfile.set_len(last_good_offset as u64)?;
                wfile.sync_all()?;
                return Ok((records, true));
            }
        }
    }

    Ok((records, false))
}

/// 只读顺序解析 segment 字节；遇错即停，**不修改**任何文件。
pub fn scan_segment_bytes(data: &[u8]) -> SegmentScanResult {
    let mut records = Vec::new();
    let mut offset = 0usize;
    let file_len = data.len();
    while offset < file_len {
        match try_parse_record(data, offset) {
            Ok((record, next_offset)) => {
                records.push(record);
                offset = next_offset;
            }
            Err(e) => {
                return SegmentScanResult {
                    records,
                    tail_error_offset: Some(offset),
                    tail_error: Some(e.to_string()),
                };
            }
        }
    }
    SegmentScanResult {
        records,
        tail_error_offset: None,
        tail_error: None,
    }
}

/// mmap 只读打开单个 segment 并扫描（不存在或空文件返回空结果）。
pub fn scan_segment_file_readonly<P: AsRef<Path>>(path: P) -> Result<SegmentScanResult> {
    let path = path.as_ref();
    if !path.exists() {
        return Ok(SegmentScanResult {
            records: Vec::new(),
            tail_error_offset: None,
            tail_error: None,
        });
    }

    let file = File::open(path)?;
    let file_len = file.metadata()?.len() as usize;
    if file_len == 0 {
        return Ok(SegmentScanResult {
            records: Vec::new(),
            tail_error_offset: None,
            tail_error: None,
        });
    }

    let mmap = unsafe { Mmap::map(&file)? };
    Ok(scan_segment_bytes(&mmap[..]))
}

/// 只沿 header 链走一遍，返回本段记录 index 的最大值；空段返回 `None`。
///
/// 与 [`scan_segment_file_readonly`] 的区别是**不校验 CRC、不解压 payload、不分配**：
/// truncate 只需要判断「本段是否全部 ≤ seq」，为此把每条记录 LZ4 解压再拷进 Vec 是纯浪费
/// （一个段可能有几万条记录）。遇到 magic/version 不符或字节不足即停，语义与全量扫描的
/// 尾部截断容错一致——半条记录不参与判断。
pub fn scan_segment_max_index<P: AsRef<Path>>(path: P) -> Result<Option<u64>> {
    let path = path.as_ref();
    if !path.exists() {
        return Ok(None);
    }
    let file = File::open(path)?;
    if file.metadata()?.len() == 0 {
        return Ok(None);
    }
    let mmap = unsafe { Mmap::map(&file)? };
    let data = &mmap[..];

    let mut max: Option<u64> = None;
    let mut offset = 0usize;
    while offset + WAL_HEADER_SIZE <= data.len() {
        let header = &data[offset..offset + WAL_HEADER_SIZE];
        if header[0..2] != WAL_MAGIC || header[2] != WAL_VERSION {
            break;
        }
        let index = u64::from_le_bytes(header[16..24].try_into()?);
        let payload_len = u32::from_le_bytes(header[24..28].try_into()?) as usize;
        let next = offset + WAL_HEADER_SIZE + payload_len;
        if next > data.len() {
            break; // 尾部半条记录：不计入
        }
        max = Some(max.map_or(index, |m: u64| m.max(index)));
        offset = next;
    }
    Ok(max)
}

fn try_parse_record(data: &[u8], offset: usize) -> Result<(WalRecord, usize)> {
    let remaining = data.len().saturating_sub(offset);
    if remaining < 3 {
        anyhow::bail!("insufficient bytes for magic/version: {remaining}");
    }

    let magic = &data[offset..offset + 2];
    if magic != WAL_MAGIC {
        anyhow::bail!("bad magic: [{:#04X}, {:#04X}]", magic[0], magic[1]);
    }

    let ver = data[offset + 2];
    if ver != WAL_VERSION {
        anyhow::bail!("unsupported WAL version: {ver} (expected {})", WAL_VERSION);
    }

    if remaining < WAL_HEADER_SIZE {
        anyhow::bail!("insufficient bytes for header: {remaining} < {WAL_HEADER_SIZE}");
    }

    let header = &data[offset..offset + WAL_HEADER_SIZE];
    let flag = header[3];
    let term = u64::from_le_bytes(header[8..16].try_into()?);
    let index = u64::from_le_bytes(header[16..24].try_into()?);
    let payload_len = u32::from_le_bytes(header[24..28].try_into()?) as usize;
    let stored_crc = u32::from_le_bytes(header[28..32].try_into()?);

    let payload_start = offset + WAL_HEADER_SIZE;
    let payload_end = payload_start + payload_len;

    if payload_end > data.len() {
        anyhow::bail!(
            "record at offset {offset} extends beyond file: need {payload_end}, have {}",
            data.len()
        );
    }

    let payload = &data[payload_start..payload_end];
    let expected_crc = crc32c_append(crc32c_append(0, &header[0..28]), payload);

    if stored_crc != expected_crc {
        anyhow::bail!(
            "CRC32C mismatch at index {index}: stored={stored_crc:#010X}, expected={expected_crc:#010X}"
        );
    }

    let entry_type = WalEntryType::from_flag(flag)?;
    let decoded = decode_payload(flag, payload)?;

    Ok((
        WalRecord {
            entry_type,
            term,
            index,
            payload: decoded,
        },
        payload_end,
    ))
}
