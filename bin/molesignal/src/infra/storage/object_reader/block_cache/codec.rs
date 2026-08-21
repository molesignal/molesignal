// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::io;

use bytes::{BufMut, Bytes, BytesMut};

const MAGIC: &[u8; 8] = b"MSBLK001";
const HEADER_LEN: usize = 8 + 8 + 32;

pub(super) fn encode_block(bytes: &Bytes) -> Bytes {
    let digest = blake3::hash(bytes);
    let mut encoded = BytesMut::with_capacity(HEADER_LEN + bytes.len());
    encoded.put_slice(MAGIC);
    encoded.put_u64_le(bytes.len() as u64);
    encoded.put_slice(digest.as_bytes());
    encoded.put_slice(bytes);
    encoded.freeze()
}

pub(super) fn decode_block(raw: &[u8], expected: usize) -> io::Result<Bytes> {
    if raw.len() < HEADER_LEN || &raw[..8] != MAGIC {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "bad block header",
        ));
    }
    let length = usize::try_from(u64::from_le_bytes(
        raw[8..16].try_into().expect("eight-byte length"),
    ))
    .map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "block length exceeds address space",
        )
    })?;
    if length != expected || raw.len() != HEADER_LEN + length {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "block length mismatch",
        ));
    }
    let payload = &raw[HEADER_LEN..];
    if blake3::hash(payload).as_bytes() != &raw[16..48] {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "block checksum mismatch",
        ));
    }
    Ok(Bytes::copy_from_slice(payload))
}

pub(super) fn verify_object_checksum(checksum: &str, bytes: &[u8]) -> object_store::Result<()> {
    let Some(expected) = checksum.strip_prefix("b3:") else {
        return Ok(());
    };
    if blake3::hash(bytes).to_hex().as_str() != expected {
        return Err(object_store::Error::Generic {
            store: "CachedObjectStore",
            source: Box::new(io::Error::other("origin object checksum mismatch")),
        });
    }
    Ok(())
}
