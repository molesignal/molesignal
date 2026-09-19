// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Debug artifact parsing and stack-frame symbolication.

use std::io::Read;

use flate2::read::GzDecoder;
use serde::{Deserialize, Serialize};

use crate::{
    domain::rum::DebugArtifactKind,
    shared::{Error, Result},
};

mod android_mapping;
mod javascript;
mod native;

pub use android_mapping::{AndroidMapping, AndroidOriginalFrame};
pub use javascript::JavascriptSourceMap;
pub use native::NativeSymbolicator;

const MAX_DECOMPRESSED_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OriginalFrame {
    pub file: Option<String>,
    pub function: Option<String>,
    pub line: Option<u32>,
    pub column: Option<u32>,
}

/// Decode a gzip artifact with a strict expanded-size bound.
pub fn decode_artifact(filename: &str, bytes: &[u8]) -> Result<Vec<u8>> {
    let gzip = filename.to_ascii_lowercase().ends_with(".gz") || bytes.starts_with(&[0x1f, 0x8b]);
    if !gzip {
        if bytes.len() as u64 > MAX_DECOMPRESSED_BYTES {
            return Err(Error::invalid(
                "debug artifact exceeds 256 MiB expanded limit",
            ));
        }
        return Ok(bytes.to_vec());
    }
    let mut decoded = Vec::new();
    GzDecoder::new(bytes)
        .take(MAX_DECOMPRESSED_BYTES + 1)
        .read_to_end(&mut decoded)
        .map_err(|error| Error::invalid(format!("gzip debug artifact: {error}")))?;
    if decoded.len() as u64 > MAX_DECOMPRESSED_BYTES {
        return Err(Error::invalid(
            "debug artifact expands beyond the 256 MiB limit",
        ));
    }
    Ok(decoded)
}

pub fn validate_artifact(kind: DebugArtifactKind, filename: &str, bytes: &[u8]) -> Result<()> {
    let decoded = decode_artifact(filename, bytes)?;
    match kind {
        DebugArtifactKind::JavascriptSourcemap => javascript::validate(&decoded),
        DebugArtifactKind::AndroidMapping => android_mapping::validate(&decoded),
        DebugArtifactKind::FlutterSymbols
        | DebugArtifactKind::AndroidNativeSymbols
        | DebugArtifactKind::AppleDsym => native::validate(&decoded),
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use flate2::{Compression, write::GzEncoder};

    use super::*;

    #[test]
    fn decodes_gzip_by_magic_without_trusting_filename() {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::fast());
        encoder.write_all(b"artifact").expect("compress");
        let encoded = encoder.finish().expect("finish");
        assert_eq!(
            decode_artifact("symbols.bin", &encoded).unwrap(),
            b"artifact"
        );
    }
}
