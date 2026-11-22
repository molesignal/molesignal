// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The bytes of one WAL object: the WAL records of one group commit.
//!
//! ```text
//! "MSW1" | flags u8 | body | crc32c(everything before) u32 LE
//! body   = count u32 LE | (len u32 LE | record)*          flags = 0
//!        = nonce(12) | AES-GCM(count | (len | record)*)   flags = 1 (sealed with the KEK)
//! ```
//!
//! Each record is one [`super::super::wal_record`] payload (an Arrow IPC batch). The checksum
//! tells a damaged object from a valid one even when it is not sealed.

use anyhow::{Result, anyhow, bail};
use postgres::cipher::CipherRootKey;

const MAGIC: &[u8; 4] = b"MSW1";
const SEALED: u8 = 1;
const NONCE_LEN: usize = 12;

/// Encodes `records` as one WAL object, sealed when `cipher` is given.
pub fn encode<'a>(
    records: impl IntoIterator<Item = &'a [u8]>,
    cipher: Option<&CipherRootKey>,
) -> Result<Vec<u8>> {
    let records: Vec<&[u8]> = records.into_iter().collect();
    let size = 4 + records.iter().map(|record| 4 + record.len()).sum::<usize>();
    let mut body = Vec::with_capacity(size);
    body.extend_from_slice(&u32::try_from(records.len())?.to_le_bytes());

    for record in records {
        body.extend_from_slice(&u32::try_from(record.len())?.to_le_bytes());
        body.extend_from_slice(record);
    }

    let mut out = Vec::with_capacity(body.len() + NONCE_LEN + 32);
    out.extend_from_slice(MAGIC);

    match cipher {
        Some(key) => {
            let (nonce, sealed) = key
                .seal(&body)
                .map_err(|error| anyhow!("wal seal: {error}"))?;

            if nonce.len() != NONCE_LEN {
                bail!("wal seal produced a {}-byte nonce", nonce.len());
            }

            out.push(SEALED);
            out.extend_from_slice(&nonce);
            out.extend_from_slice(&sealed);
        }
        None => {
            out.push(0);
            out.extend_from_slice(&body);
        }
    }

    let checksum = crc32c::crc32c(&out);
    out.extend_from_slice(&checksum.to_le_bytes());
    Ok(out)
}

/// The records of a WAL object. A damaged object, or a sealed one without the key, is an error.
pub fn decode(bytes: &[u8], cipher: Option<&CipherRootKey>) -> Result<Vec<Vec<u8>>> {
    if bytes.len() < MAGIC.len() + 1 + 4 || &bytes[..MAGIC.len()] != MAGIC {
        bail!("not a WAL object");
    }

    let (framed, checksum) = bytes.split_at(bytes.len() - 4);
    let expected = u32::from_le_bytes(checksum.try_into()?);

    if crc32c::crc32c(framed) != expected {
        bail!("WAL object checksum mismatch");
    }

    let flags = framed[MAGIC.len()];
    let payload = &framed[MAGIC.len() + 1..];

    let opened;

    let body = match flags {
        0 => payload,
        SEALED => {
            let key = cipher.ok_or_else(|| anyhow!("WAL object is sealed but no key is set"))?;

            if payload.len() < NONCE_LEN {
                bail!("sealed WAL object too short");
            }

            let (nonce, sealed) = payload.split_at(NONCE_LEN);
            opened = key
                .open(nonce, sealed)
                .map_err(|error| anyhow!("wal open: {error}"))?;
            opened.as_slice()
        }
        other => bail!("unknown WAL object flags {other}"),
    };

    read_records(body)
}

fn read_records(body: &[u8]) -> Result<Vec<Vec<u8>>> {
    let mut rest = body;
    let count = take_u32(&mut rest)? as usize;
    let mut records = Vec::with_capacity(count.min(4096));

    for _ in 0..count {
        let len = take_u32(&mut rest)? as usize;

        if rest.len() < len {
            bail!("WAL object record runs past its end");
        }

        let (record, tail) = rest.split_at(len);
        records.push(record.to_vec());
        rest = tail;
    }

    if !rest.is_empty() {
        bail!("WAL object has {} trailing bytes", rest.len());
    }

    Ok(records)
}

fn take_u32(rest: &mut &[u8]) -> Result<u32> {
    if rest.len() < 4 {
        bail!("WAL object truncated");
    }

    let (head, tail) = rest.split_at(4);
    *rest = tail;
    Ok(u32::from_le_bytes(head.try_into()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> CipherRootKey {
        CipherRootKey::from_base64("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=").unwrap()
    }

    #[test]
    fn records_round_trip_plain_and_sealed() {
        let records: [&[u8]; 3] = [b"first", b"", b"third record"];

        let plain = encode(records, None).unwrap();
        assert_eq!(decode(&plain, None).unwrap(), records.map(<[u8]>::to_vec));

        let sealed = encode(records, Some(&key())).unwrap();
        assert!(!sealed.windows(5).any(|window| window == b"first"));
        assert_eq!(
            decode(&sealed, Some(&key())).unwrap(),
            records.map(<[u8]>::to_vec)
        );
        assert!(
            decode(&sealed, None).is_err(),
            "a sealed object needs the key"
        );
    }

    #[test]
    fn a_damaged_or_foreign_object_is_refused() {
        let mut bytes = encode([b"payload".as_slice()], None).unwrap();
        let middle = bytes.len() / 2;
        bytes[middle] ^= 0x40;
        assert!(
            decode(&bytes, None)
                .unwrap_err()
                .to_string()
                .contains("checksum")
        );

        assert!(decode(b"PAR1....", None).is_err());
        let whole = encode([b"payload".as_slice()], None).unwrap();
        assert!(decode(&whole[..whole.len() - 1], None).is_err());
    }
}
