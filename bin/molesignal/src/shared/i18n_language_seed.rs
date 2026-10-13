// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The `i18n_language` entries a fresh database starts with, for tests that
//! check what the backend writes without a database.

use std::collections::HashMap;

use crate::domain::i18n_language::{FALLBACK_LANGUAGE_CODE, LanguagePack};

/// The seeded entries under `key_prefix`, read the way the repository reads
/// them: `language_code` first, the fallback language for what it lacks.
pub fn seeded_pack(key_prefix: &str, language_code: &str) -> LanguagePack {
    let baseline = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../crates/engines/postgres/src/migrations/20260101000001_initial.sql"
    ))
    .expect("read the baseline migration");

    let (_, seed) = baseline
        .split_once("INSERT INTO i18n_language")
        .expect("i18n_language seed");

    // One seed row per line. The seed escapes nothing but a line break.
    let row =
        regex::Regex::new(r"(?m)^\s*\('([a-z0-9_.]+)', '([a-z0-9-]+)', (E?)'((?:[^']|'')*)'\),?$")
            .expect("valid seed row pattern");

    let mut own = HashMap::new();
    let mut fallback = HashMap::new();

    for seeded in row.captures_iter(seed) {
        if !seeded[1].starts_with(key_prefix) {
            continue;
        }

        let mut value = seeded[4].replace("''", "'");

        if &seeded[3] == "E" {
            value = value.replace("\\n", "\n");
        }

        if &seeded[2] == language_code {
            own.insert(seeded[1].to_string(), value);
        } else if &seeded[2] == FALLBACK_LANGUAGE_CODE {
            fallback.insert(seeded[1].to_string(), value);
        }
    }

    fallback.extend(own);
    LanguagePack::new(fallback)
}
