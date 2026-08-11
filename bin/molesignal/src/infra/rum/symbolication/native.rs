// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::io::Write;

use addr2line::Loader;
use tempfile::NamedTempFile;

use super::OriginalFrame;
use crate::shared::{Error, Result};

pub struct NativeSymbolicator {
    _file: NamedTempFile,
    loader: Loader,
}

impl NativeSymbolicator {
    pub fn new(bytes: &[u8]) -> Result<Self> {
        let mut file = NamedTempFile::new()
            .map_err(|error| Error::internal(format!("debug artifact temp file: {error}")))?;
        file.write_all(bytes)
            .map_err(|error| Error::internal(format!("debug artifact temp write: {error}")))?;
        file.flush()
            .map_err(|error| Error::internal(format!("debug artifact temp flush: {error}")))?;
        let loader = Loader::new(file.path())
            .map_err(|error| Error::invalid(format!("native debug artifact: {error}")))?;
        Ok(Self {
            _file: file,
            loader,
        })
    }

    pub fn translate(&self, address: u64) -> Result<OriginalFrame> {
        if let Some(frame) = self.translate_at(address)? {
            return Ok(frame);
        }
        // Mobile SDKs normally submit an image-relative address. DWARF may use
        // the linked text virtual address, especially for Mach-O dSYM files.
        for section in [b"__text".as_slice(), b".text".as_slice()] {
            if let Some(range) = self.loader.get_section_range(section)
                && let Some(probe) = range.begin.checked_add(address)
                && let Some(frame) = self.translate_at(probe)?
            {
                return Ok(frame);
            }
        }
        Ok(OriginalFrame::default())
    }

    fn translate_at(&self, address: u64) -> Result<Option<OriginalFrame>> {
        let mut frames = self
            .loader
            .find_frames(address)
            .map_err(|error| Error::invalid(format!("native frame lookup: {error}")))?;
        if let Some(frame) = frames
            .next()
            .map_err(|error| Error::invalid(format!("native frame decode: {error}")))?
        {
            let function = frame
                .function
                .and_then(|name| name.demangle().ok().map(|value| value.into_owned()))
                .or_else(|| self.loader.find_symbol(address).map(String::from));
            let location = frame.location;
            return Ok(Some(OriginalFrame {
                file: location
                    .as_ref()
                    .and_then(|value| value.file.map(String::from)),
                function,
                line: location.as_ref().and_then(|value| value.line),
                column: location.and_then(|value| value.column),
            }));
        }
        Ok(self
            .loader
            .find_symbol(address)
            .map(|symbol| OriginalFrame {
                function: Some(symbol.to_string()),
                ..OriginalFrame::default()
            }))
    }
}

pub fn validate(bytes: &[u8]) -> Result<()> {
    NativeSymbolicator::new(bytes).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_object_artifacts() {
        assert!(NativeSymbolicator::new(b"not an ELF or Mach-O file").is_err());
    }
}
