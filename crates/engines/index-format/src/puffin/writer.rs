// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Puffin v1 writer：把多 blob 顺序追加进一个 buffer，最后 `finish()` 写入 footer。

use std::{collections::HashMap, io::Write};

use anyhow::{Result, anyhow};

use super::{BlobMetadata, BlobTypes, FLAGS_SIZE, MAGIC, PuffinFooterFlags, PuffinMeta};

pub struct PuffinBytesWriter<W: Write> {
    buf: W,
    /// 起始 MAGIC 已写入字节数（4 字节）+ 已写 blob 字节数。
    next_offset: u64,
    blobs: Vec<BlobMetadata>,
    properties: HashMap<String, String>,
    started: bool,
    finished: bool,
}

impl<W: Write> PuffinBytesWriter<W> {
    pub fn new(buf: W) -> Self {
        Self {
            buf,
            next_offset: 0,
            blobs: Vec::new(),
            properties: HashMap::new(),
            started: false,
            finished: false,
        }
    }

    pub fn set_property(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.properties.insert(key.into(), value.into());
    }

    fn ensure_started(&mut self) -> Result<()> {
        if self.started {
            return Ok(());
        }
        self.buf.write_all(&MAGIC)?;
        self.next_offset += MAGIC.len() as u64;
        self.started = true;
        Ok(())
    }

    /// Append 一个 blob。`blob_tag` 写到 `properties["blob_tag"]`，
    /// reader 端用 blob_tag 把 blob 关联到 tantivy 文件路径。
    pub fn add_blob(
        &mut self,
        bytes: &[u8],
        blob_type: BlobTypes,
        blob_tag: impl Into<String>,
    ) -> Result<()> {
        if self.finished {
            return Err(anyhow!("PuffinBytesWriter already finished"));
        }
        self.ensure_started()?;
        let offset = self.next_offset;
        self.buf.write_all(bytes)?;
        self.next_offset += bytes.len() as u64;
        let mut props = HashMap::new();
        props.insert("blob_tag".to_string(), blob_tag.into());
        self.blobs.push(BlobMetadata {
            blob_type,
            fields: Vec::new(),
            snapshot_id: 0,
            sequence_number: 0,
            offset,
            length: bytes.len() as u64,
            compression_codec: None,
            properties: props,
        });
        Ok(())
    }

    /// 写入 footer：内圈 MAGIC + JSON payload + payload_size + flags + 外圈 MAGIC。
    pub fn finish(mut self) -> Result<()> {
        if self.finished {
            return Err(anyhow!("PuffinBytesWriter already finished"));
        }
        self.ensure_started()?;
        let meta = PuffinMeta {
            blobs: std::mem::take(&mut self.blobs),
            properties: std::mem::take(&mut self.properties),
        };
        let payload =
            serde_json::to_vec(&meta).map_err(|e| anyhow!("PuffinMeta serialize: {e}"))?;
        // inner MAGIC (mark start of footer payload)
        self.buf.write_all(&MAGIC)?;
        // payload
        self.buf.write_all(&payload)?;
        // payload_size (u32 LE)
        let payload_size = payload.len() as u32;
        self.buf.write_all(&payload_size.to_le_bytes())?;
        // flags (u32 LE)
        let flags = PuffinFooterFlags::DEFAULT.bits();
        self.buf.write_all(&flags.to_le_bytes())?;
        // trailing MAGIC
        self.buf.write_all(&MAGIC)?;
        let _ = FLAGS_SIZE;
        self.finished = true;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_single_blob_roundtrip_offsets() {
        let mut buf: Vec<u8> = Vec::new();
        {
            let mut w = PuffinBytesWriter::new(&mut buf);
            w.add_blob(b"hello", BlobTypes::TantivySegmentV1, "f1")
                .unwrap();
            w.finish().unwrap();
        }
        // first 4 bytes magic, then 5 bytes blob, then footer.
        assert_eq!(&buf[..4], &MAGIC[..]);
        assert_eq!(&buf[4..9], b"hello");
        assert_eq!(&buf[buf.len() - 4..], &MAGIC[..]);
    }

    #[test]
    fn write_multiple_blobs_offsets_monotonic() {
        let mut buf: Vec<u8> = Vec::new();
        {
            let mut w = PuffinBytesWriter::new(&mut buf);
            w.add_blob(b"a", BlobTypes::TantivySegmentV1, "a").unwrap();
            w.add_blob(b"bb", BlobTypes::TantivySegmentV1, "b").unwrap();
            w.add_blob(b"ccc", BlobTypes::TantivyFooterV1, "c").unwrap();
            w.finish().unwrap();
        }
        // offsets relative to start of file: 4 (after magic) / 5 / 7
        let meta_start = 4 + 1 + 2 + 3 /*blobs*/ + 4 /*inner magic*/;
        let payload_size_bytes = &buf[buf.len() - 4 - 4 - 4..buf.len() - 4 - 4];
        let payload_size = u32::from_le_bytes(payload_size_bytes.try_into().unwrap()) as usize;
        let payload = &buf[meta_start..meta_start + payload_size];
        let meta: PuffinMeta = serde_json::from_slice(payload).unwrap();
        assert_eq!(meta.blobs.len(), 3);
        assert_eq!(meta.blobs[0].offset, 4);
        assert_eq!(meta.blobs[1].offset, 5);
        assert_eq!(meta.blobs[2].offset, 7);
        assert_eq!(meta.blobs[0].length, 1);
        assert_eq!(meta.blobs[1].length, 2);
        assert_eq!(meta.blobs[2].length, 3);
        assert_eq!(meta.blobs[0].properties.get("blob_tag").unwrap(), "a");
    }

    #[test]
    fn writer_set_property_round_trips() {
        let mut buf: Vec<u8> = Vec::new();
        {
            let mut w = PuffinBytesWriter::new(&mut buf);
            w.set_property("schema_version", "1");
            w.add_blob(b"x", BlobTypes::TantivySegmentV1, "f").unwrap();
            w.finish().unwrap();
        }
        // crude: locate JSON payload and re-parse.
        let payload_size =
            u32::from_le_bytes(buf[buf.len() - 12..buf.len() - 8].try_into().unwrap()) as usize;
        let meta_start = buf.len() - 4 - 4 - 4 - payload_size;
        let payload = &buf[meta_start..meta_start + payload_size];
        let meta: PuffinMeta = serde_json::from_slice(payload).unwrap();
        assert_eq!(meta.properties.get("schema_version").unwrap(), "1");
    }
}
