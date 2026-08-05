// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::HashSet;

use serde_json::{Map, Value};

use crate::shared::{
    Error, Result,
    contracts::{ContractValidator, canonical_json_bytes, sha256_hex},
};

pub(super) const DIALECT_2020_12: &str = "https://json-schema.org/draft/2020-12/schema";
const MAX_SCHEMA_BYTES: usize = 128 * 1024;
const MAX_SCHEMA_DEPTH: usize = 32;
const MAX_REFERENCE_DEPTH: usize = 16;
const MAX_REFERENCES: usize = 256;

pub(super) struct SynchronizedSchema {
    pub schema: Value,
    pub hash: String,
    pub dialect: String,
}

pub(super) fn synchronize(mut schema: Value) -> Result<SynchronizedSchema> {
    if !schema.is_object() {
        return Err(Error::invalid("MCP input schema must be a JSON object"));
    }
    enforce_size_and_depth(&schema)?;
    let dialect = normalize_dialect(&mut schema)?;
    normalize_object_root(&mut schema);
    validate_references(&schema)?;
    ContractValidator::compile(schema.clone())
        .map_err(|error| Error::invalid(format!("MCP input schema is invalid: {error}")))?;
    let hash = sha256_hex(canonical_json_bytes(&schema));
    Ok(SynchronizedSchema {
        schema,
        hash,
        dialect,
    })
}

pub(crate) fn validate_schema(schema: &Value, input: &Value) -> Result<()> {
    let synchronized = synchronize(schema.clone())?;
    validate_compiled(&synchronized.schema, input)
}

pub(super) fn validate_schema_revision(
    schema: &Value,
    expected_hash: &str,
    input: &Value,
) -> Result<()> {
    let synchronized = synchronize(schema.clone())?;
    if synchronized.hash != expected_hash {
        return Err(Error::conflict(
            "MCP tool schema revision changed; synchronize the server before execution",
        ));
    }
    validate_compiled(&synchronized.schema, input)
}

fn validate_compiled(schema: &Value, input: &Value) -> Result<()> {
    if !input.is_object() {
        return Err(Error::invalid("tool arguments must be a JSON object"));
    }
    let validator = ContractValidator::compile(schema.clone())
        .map_err(|error| Error::invalid(format!("MCP input schema is invalid: {error}")))?;
    let issues = validator.validate(input);
    if issues.is_empty() {
        Ok(())
    } else {
        Err(Error::validation(
            "MCP tool arguments do not match the synchronized input schema",
            issues,
        ))
    }
}

fn normalize_dialect(schema: &mut Value) -> Result<String> {
    let dialect = schema.get("$schema").and_then(Value::as_str);
    match dialect {
        None
        | Some("https://json-schema.org/draft/2020-12/schema")
        | Some("https://json-schema.org/draft/2020-12/schema#")
        | Some("http://json-schema.org/draft/2020-12/schema")
        | Some("http://json-schema.org/draft/2020-12/schema#") => {
            schema["$schema"] = Value::String(DIALECT_2020_12.into());
            Ok(DIALECT_2020_12.into())
        }
        Some(other) => Err(Error::invalid(format!(
            "unsupported MCP JSON Schema dialect `{other}`"
        ))),
    }
}

fn normalize_object_root(schema: &mut Value) {
    if schema.get("type").is_none()
        && schema.get("oneOf").is_none()
        && schema.get("anyOf").is_none()
        && schema.get("allOf").is_none()
    {
        schema["type"] = Value::String("object".into());
    }
    if schema.get("type").and_then(Value::as_str) == Some("object")
        && schema.get("properties").is_none()
    {
        schema["properties"] = Value::Object(Map::new());
    }
}

fn enforce_size_and_depth(schema: &Value) -> Result<()> {
    let bytes = serde_json::to_vec(schema)
        .map_err(|error| Error::invalid(format!("MCP schema serialization failed: {error}")))?;
    if bytes.len() > MAX_SCHEMA_BYTES {
        return Err(Error::invalid(format!(
            "MCP input schema exceeds {MAX_SCHEMA_BYTES} bytes"
        )));
    }
    fn visit(value: &Value, depth: usize) -> Result<()> {
        if depth > MAX_SCHEMA_DEPTH {
            return Err(Error::invalid(format!(
                "MCP input schema exceeds nesting depth {MAX_SCHEMA_DEPTH}"
            )));
        }
        match value {
            Value::Array(values) => {
                for value in values {
                    visit(value, depth + 1)?;
                }
            }
            Value::Object(values) => {
                for value in values.values() {
                    visit(value, depth + 1)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    visit(schema, 0)
}

fn validate_references(schema: &Value) -> Result<()> {
    let mut refs = Vec::new();
    collect_refs(schema, &mut refs);
    if refs.len() > MAX_REFERENCES {
        return Err(Error::invalid(format!(
            "MCP input schema exceeds {MAX_REFERENCES} references"
        )));
    }
    for reference in refs {
        if !reference.starts_with("#/") {
            return Err(Error::invalid(format!(
                "MCP schema reference `{reference}` is not a local JSON Pointer"
            )));
        }
        let mut visiting = HashSet::new();
        follow_reference(schema, reference, 1, &mut visiting)?;
    }
    Ok(())
}

fn collect_refs<'a>(value: &'a Value, refs: &mut Vec<&'a str>) {
    match value {
        Value::Object(values) => {
            if let Some(reference) = values.get("$ref").and_then(Value::as_str) {
                refs.push(reference);
            }
            for value in values.values() {
                collect_refs(value, refs);
            }
        }
        Value::Array(values) => {
            for value in values {
                collect_refs(value, refs);
            }
        }
        _ => {}
    }
}

fn follow_reference(
    root: &Value,
    reference: &str,
    depth: usize,
    visiting: &mut HashSet<String>,
) -> Result<()> {
    if depth > MAX_REFERENCE_DEPTH {
        return Err(Error::invalid(format!(
            "MCP schema reference depth exceeds {MAX_REFERENCE_DEPTH}"
        )));
    }
    if !visiting.insert(reference.to_string()) {
        return Err(Error::invalid(
            "recursive MCP schema references are not supported",
        ));
    }
    let pointer = reference
        .strip_prefix('#')
        .ok_or_else(|| Error::invalid("MCP schema reference must be local"))?;
    let target = root.pointer(pointer).ok_or_else(|| {
        Error::invalid(format!(
            "MCP schema reference `{reference}` does not resolve"
        ))
    })?;
    let mut nested = Vec::new();
    collect_refs(target, &mut nested);
    for reference in nested {
        follow_reference(root, reference, depth + 1, visiting)?;
    }
    visiting.remove(reference);
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn validates_nested_composition_and_local_references() {
        let schema = json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["items", "mode", "limit"],
            "properties": {
                "items": {"type": "array", "minItems": 1, "items": {"$ref": "#/$defs/item"}},
                "mode": {"enum": ["fast", "safe"]},
                "limit": {"type": "integer", "minimum": 1, "maximum": 100}
            },
            "$defs": {
                "item": {
                    "oneOf": [
                        {"type": "object", "required": ["kind"], "properties": {"kind": {"const": "metric"}}},
                        {"type": "object", "required": ["kind"], "properties": {"kind": {"const": "log"}}}
                    ]
                }
            }
        });
        let valid = json!({"items": [{"kind": "metric"}], "mode": "safe", "limit": 25});
        assert!(validate_schema(&schema, &valid).is_ok());
        assert!(validate_schema(&schema, &json!({"mode": "safe", "limit": 25})).is_err());
        assert!(
            validate_schema(
                &schema,
                &json!({"items": [{"kind": "trace"}], "mode": "safe", "limit": 25})
            )
            .is_err()
        );
        assert!(
            validate_schema(&schema, &json!({"items": [], "mode": "safe", "limit": 25})).is_err()
        );
        assert!(
            validate_schema(
                &schema,
                &json!({"items": [{"kind": "log"}], "mode": "unsafe", "limit": 25})
            )
            .is_err()
        );
        assert!(
            validate_schema(
                &schema,
                &json!({"items": [{"kind": "log"}], "mode": "fast", "limit": 101})
            )
            .is_err()
        );
        let mut extra = valid;
        extra["extra"] = json!(true);
        assert!(validate_schema(&schema, &extra).is_err());
    }

    #[test]
    fn rejects_remote_or_unsupported_schema() {
        assert!(synchronize(json!({"$ref": "https://example.com/schema"})).is_err());
        assert!(
            synchronize(json!({"$schema": "http://json-schema.org/draft-07/schema#"})).is_err()
        );
        assert!(synchronize(json!({"type": 42})).is_err());
        assert!(synchronize(json!({"$ref": "#/$defs/missing"})).is_err());
    }

    #[test]
    fn rejects_oversized_deep_recursive_or_reference_heavy_schemas() {
        assert!(
            synchronize(json!({
                "type": "object",
                "description": "x".repeat(MAX_SCHEMA_BYTES)
            }))
            .is_err()
        );

        let mut deeply_nested = json!({"type": "string"});
        for _ in 0..=MAX_SCHEMA_DEPTH {
            deeply_nested = json!({"allOf": [deeply_nested]});
        }
        assert!(synchronize(deeply_nested).is_err());

        assert!(
            synchronize(json!({
                "$ref": "#/$defs/node",
                "$defs": {"node": {"$ref": "#/$defs/node"}}
            }))
            .is_err()
        );
        let mut definitions = Map::new();
        for index in (0..=MAX_REFERENCE_DEPTH).rev() {
            let target = if index == MAX_REFERENCE_DEPTH {
                json!({"type": "string"})
            } else {
                json!({"$ref": format!("#/$defs/ref-{}", index + 1)})
            };
            definitions.insert(format!("ref-{index}"), target);
        }
        assert!(
            synchronize(json!({
                "$ref": "#/$defs/ref-0",
                "$defs": definitions
            }))
            .is_err()
        );
        let references = (0..=MAX_REFERENCES)
            .map(|_| json!({"$ref": "#/$defs/value"}))
            .collect::<Vec<_>>();
        assert!(
            synchronize(json!({
                "allOf": references,
                "$defs": {"value": {"type": "string"}}
            }))
            .is_err()
        );
    }

    #[test]
    fn canonical_hash_tracks_schema_revisions() {
        let first = synchronize(json!({
            "required": ["query"],
            "properties": {"query": {"type": "string"}}
        }))
        .unwrap();
        let reordered = synchronize(json!({
            "properties": {"query": {"type": "string"}},
            "required": ["query"]
        }))
        .unwrap();
        let revised = synchronize(json!({
            "required": ["query"],
            "properties": {"query": {"type": "string", "minLength": 1}}
        }))
        .unwrap();
        assert_eq!(first.hash, reordered.hash);
        assert_ne!(first.hash, revised.hash);
        assert!(
            validate_schema_revision(&revised.schema, &first.hash, &json!({"query": "up"}))
                .is_err()
        );
    }
}
