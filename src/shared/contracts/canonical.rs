// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

/// Returns a recursively key-sorted JSON value suitable for stable hashing.
#[must_use]
pub fn canonical_json(value: &Value) -> Value {
    match value {
        Value::Array(values) => Value::Array(values.iter().map(canonical_json).collect()),
        Value::Object(values) => {
            let mut keys = values.keys().collect::<Vec<_>>();
            keys.sort_unstable();
            let mut canonical = Map::with_capacity(values.len());
            for key in keys {
                canonical.insert(key.clone(), canonical_json(&values[key]));
            }
            Value::Object(canonical)
        }
        scalar => scalar.clone(),
    }
}

/// Serializes JSON without insignificant whitespace after recursively sorting keys.
#[must_use]
pub fn canonical_json_bytes(value: &Value) -> Vec<u8> {
    serde_json::to_vec(&canonical_json(value))
        .expect("serializing an in-memory serde_json::Value cannot fail")
}

/// Computes a lowercase SHA-256 digest for arbitrary bytes.
#[must_use]
pub fn sha256_hex(bytes: impl AsRef<[u8]>) -> String {
    hex::encode(Sha256::digest(bytes.as_ref()))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn canonical_form_sorts_nested_object_keys() {
        let left = json!({"z": [{"b": 2, "a": 1}], "a": true});
        let right = json!({"a": true, "z": [{"a": 1, "b": 2}]});

        assert_eq!(canonical_json_bytes(&left), canonical_json_bytes(&right));
        assert_eq!(
            sha256_hex(canonical_json_bytes(&left)),
            sha256_hex(canonical_json_bytes(&right))
        );
    }
}
