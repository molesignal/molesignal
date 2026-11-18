// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Counting a column's values from the dictionary of the exact index of each file inside the
//! window.

use std::collections::HashMap;

use domain::storage::QueryFile;
use kernel::time::TimeRange;

use super::{DataFusionEngine, window::within};

/// 整个落在 `window` 里的文件用整值索引的词典数出 `column` 每个值的行数，`accept` 收下的加进
/// `counts`。返回数出的文件，与其余仍要扫描的文件。
pub(super) async fn count_values(
    engine: &DataFusionEngine,
    files: Vec<QueryFile>,
    window: TimeRange,
    column: &str,
    index_keys: &HashMap<String, String>,
    counts: &mut HashMap<String, u64>,
    mut accept: impl FnMut(&QueryFile, &HashMap<String, u64>) -> bool,
) -> (Vec<QueryFile>, Vec<QueryFile>) {
    let Some(pruner) = &engine.tantivy_pruner else {
        return (Vec::new(), files);
    };

    let (covered, mut rest): (Vec<QueryFile>, Vec<QueryFile>) = files
        .into_iter()
        .partition(|file| within(window, file.time_range));

    let counted = pruner.count_values(&covered, column, index_keys).await;
    let mut taken = Vec::new();

    for (file, counted) in covered.into_iter().zip(counted) {
        let Some(counted) = counted.filter(|counted| accept(&file, counted)) else {
            rest.push(file);
            continue;
        };

        for (value, count) in counted {
            *counts.entry(value).or_default() += count;
        }

        taken.push(file);
    }

    (taken, rest)
}
