// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 非表格查询结果的字段级遮掩适配。

use std::collections::HashMap;

use super::{mask_value, normalize_identifier};
use crate::{
    domain::{masking::FieldMaskingAlgorithm, metrics::MetricLabelSet},
    infra::cipher::CipherRootKey,
    shared::ids::Id,
};

pub(super) fn mask_label_set(
    labels: &mut MetricLabelSet,
    algorithms: &HashMap<String, FieldMaskingAlgorithm>,
    root_key: &CipherRootKey,
    org_id: &Id,
) {
    for (field, value) in labels {
        let Some(algorithm) = algorithms.get(&normalize_identifier(field)) else {
            continue;
        };
        let mut json = serde_json::Value::String(value.clone());
        mask_value(&mut json, algorithm, root_key, org_id);
        *value = json.as_str().unwrap_or_default().to_string();
    }
}
