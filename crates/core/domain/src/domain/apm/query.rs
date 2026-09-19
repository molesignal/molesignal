// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde::{Deserialize, Serialize};

use crate::shared::cursor::CursorDirection;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueryResolution {
    #[default]
    Auto,
    Minute,
    Hour,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortDirection {
    Asc,
    Desc,
}

/// Decoded stable cursor. HTTP adapters encode this structure as an opaque
/// URL-safe token and validate `sort_field` against the active endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueryCursor {
    #[serde(default = "cursor_version")]
    pub version: u8,
    pub sort_field: String,
    pub direction: SortDirection,
    #[serde(default)]
    pub page_direction: CursorDirection,
    pub sort_value: String,
    pub tie_breaker: String,
}

fn cursor_version() -> u8 {
    1
}
