// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Canonical pre-transformation event bytes, carried through WAL to flush accounting.

use std::io::{self, Write};

use kernel::{Error, Result};

use super::RawEvent;

pub mod budget;
pub mod source;

/// Count UTF-8 JSON bytes without allocating a serialized copy of the payload.
/// The canonical event includes its timestamp and fields, but no transport/batch framing.
pub fn event_bytes(event: &RawEvent) -> Result<u64> {
    struct Counter(u64);

    impl Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0 = self
                .0
                .checked_add(bytes.len() as u64)
                .ok_or_else(|| io::Error::other("intake byte count overflow"))?;

            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    let mut counter = Counter(0);
    serde_json::to_writer(&mut counter, event)
        .map_err(|error| Error::internal(format!("measure intake event: {error}")))?;
    Ok(counter.0)
}

#[cfg(test)]
mod tests {
    use kernel::time::TimestampMicros;

    use super::*;

    #[test]
    fn counts_utf8_and_escaping_exactly() {
        let event = RawEvent {
            timestamp: TimestampMicros(123),
            fields: serde_json::from_value(serde_json::json!({"message":"中文\n\"hello\""}))
                .unwrap(),
        };
        assert_eq!(
            event_bytes(&event).unwrap(),
            serde_json::to_vec(&event).unwrap().len() as u64
        );
    }
}
