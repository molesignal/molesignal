// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Explicit Primary Artifact → ready Tantivy Artifact lookup.

use std::collections::HashMap;

use crate::domain::storage::QueryFile;

pub(super) fn key_for<'a>(keys: &'a HashMap<String, String>, file: &QueryFile) -> Option<&'a str> {
    keys.get(&file.object_key).map(String::as_str)
}
