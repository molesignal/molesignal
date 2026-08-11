// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use super::OriginalFrame;
use crate::shared::{Error, Result};

pub struct JavascriptSourceMap {
    decoded: sourcemap::DecodedMap,
}

impl JavascriptSourceMap {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let decoded = sourcemap::decode_slice(bytes)
            .map_err(|error| Error::invalid(format!("JavaScript source map: {error}")))?;
        Ok(Self { decoded })
    }

    pub fn translate(&self, line: u32, column: u32) -> OriginalFrame {
        let Some(token) = self
            .decoded
            .lookup_token(line.saturating_sub(1), column.saturating_sub(1))
        else {
            return OriginalFrame::default();
        };
        OriginalFrame {
            file: token.get_source().map(String::from),
            function: token.get_name().map(String::from),
            line: Some(token.get_src_line().saturating_add(1)),
            column: Some(token.get_src_col().saturating_add(1)),
        }
    }
}

pub fn validate(bytes: &[u8]) -> Result<()> {
    JavascriptSourceMap::parse(bytes).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translates_a_single_token_map() {
        let map = br#"{"version":3,"sources":["src/app.tsx"],"names":["handleClick"],"mappings":"AAAAA"}"#;
        let frame = JavascriptSourceMap::parse(map)
            .expect("parse")
            .translate(1, 1);
        assert_eq!(frame.file.as_deref(), Some("src/app.tsx"));
        assert_eq!(frame.function.as_deref(), Some("handleClick"));
        assert_eq!(frame.line, Some(1));
        assert_eq!(frame.column, Some(1));
    }

    #[test]
    fn translates_an_indexed_source_map() {
        let map = br#"{
          "version": 3,
          "sections": [{
            "offset": {"line": 0, "column": 0},
            "map": {
              "version": 3,
              "sources": ["src/main.dart"],
              "names": ["main"],
              "mappings": "AAAAA"
            }
          }]
        }"#;
        let frame = JavascriptSourceMap::parse(map)
            .expect("parse index")
            .translate(1, 1);
        assert_eq!(frame.file.as_deref(), Some("src/main.dart"));
        assert_eq!(frame.function.as_deref(), Some("main"));
    }
}
