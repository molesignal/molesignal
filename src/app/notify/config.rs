// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde_json::Value;

use super::ConnectorRegistry;
use crate::{
    domain::notify::connector::{NotifyTarget, NotifyTargetType},
    shared::{Error, Result},
};

pub(super) fn validate_name(name: &str) -> Result<()> {
    let name = name.trim();
    if name.is_empty() {
        return Err(Error::invalid("notify connector name cannot be empty"));
    }
    if name.chars().count() > 255 {
        return Err(Error::invalid(
            "notify connector name must be at most 255 characters",
        ));
    }
    Ok(())
}

pub(super) fn validate_endpoint_identity(
    registry: &ConnectorRegistry,
    connector_type: &str,
    identity: &str,
) -> Result<()> {
    registry
        .get(connector_type)?
        .validate_target(&NotifyTarget {
            target_type: NotifyTargetType::DirectUser,
            value: identity.trim().to_string(),
            metadata: Default::default(),
        })
}

pub(super) fn normalize_optional_text(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let value = value.trim();
        (!value.is_empty()).then(|| value.to_string())
    })
}

pub(super) fn normalize_object(value: Value, field: &str) -> Result<Value> {
    if value.is_object() {
        Ok(value)
    } else {
        Err(Error::invalid(format!("{field} must be a JSON object")))
    }
}

pub(super) fn truncate_error(error: &str) -> String {
    error.chars().take(2_048).collect()
}

pub(super) fn mask_target(value: &str) -> String {
    let value = value.trim();
    if let Some((local, domain)) = value.split_once('@') {
        let first = local.chars().next().unwrap_or('*');
        return format!("{first}***@{domain}");
    }
    let chars = value.chars().collect::<Vec<_>>();
    if chars.len() <= 4 {
        return "***".into();
    }
    format!(
        "{}{}***{}{}",
        chars[0],
        chars[1],
        chars[chars.len() - 2],
        chars[chars.len() - 1]
    )
}

const SENSITIVE_CONFIG_KEYS: &[&str] = &[
    "password",
    "secret",
    "token",
    "api_key",
    "access_key",
    "secret_key",
    "client_secret",
    "private_key",
    "webhook_url",
    "url",
    "headers",
];

fn sensitive_key(key: &str) -> bool {
    let normalized = key.to_ascii_lowercase();
    SENSITIVE_CONFIG_KEYS.iter().any(|candidate| {
        normalized == *candidate || (*candidate != "url" && normalized.ends_with(candidate))
    })
}

pub fn mask_connector_config(config: &Value) -> Value {
    match config {
        Value::Object(fields) => Value::Object(
            fields
                .iter()
                .map(|(key, value)| {
                    let value = if sensitive_key(key) {
                        Value::String("***".into())
                    } else {
                        mask_connector_config(value)
                    };
                    (key.clone(), value)
                })
                .collect(),
        ),
        Value::Array(values) => {
            Value::Array(values.iter().map(mask_connector_config).collect::<Vec<_>>())
        }
        other => other.clone(),
    }
}

pub(super) fn merge_masked_config(existing: &Value, desired: &Value) -> Value {
    match (existing, desired) {
        (Value::Object(existing), Value::Object(desired)) => Value::Object(
            desired
                .iter()
                .map(|(key, value)| {
                    let merged = if sensitive_key(key) && value.as_str() == Some("***") {
                        existing.get(key).cloned().unwrap_or(Value::Null)
                    } else {
                        existing
                            .get(key)
                            .map_or_else(|| value.clone(), |old| merge_masked_config(old, value))
                    };
                    (key.clone(), merged)
                })
                .collect(),
        ),
        (_, desired) => desired.clone(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn config_mask_is_recursive_and_preserves_non_secrets() {
        let config = serde_json::json!({
            "host": "smtp.example.com",
            "url": "https://hooks.example.com/secret",
            "api_base_url": "https://api.example.com",
            "auth": {"password": "secret", "username": "mailer"}
        });
        let masked = mask_connector_config(&config);
        assert_eq!(masked["host"], "smtp.example.com");
        assert_eq!(masked["url"], "***");
        assert_eq!(masked["api_base_url"], "https://api.example.com");
        assert_eq!(masked["auth"]["password"], "***");
        assert_eq!(masked["auth"]["username"], "mailer");
    }

    #[test]
    fn masked_update_keeps_existing_secret() {
        let existing = serde_json::json!({
            "host": "old.example.com",
            "password": "secret",
            "url": "https://hooks.example.com/secret",
            "api_base_url": "https://api.example.com"
        });
        let desired = serde_json::json!({
            "host": "new.example.com",
            "password": "***",
            "url": "***",
            "api_base_url": "https://new-api.example.com"
        });
        let merged = merge_masked_config(&existing, &desired);
        assert_eq!(merged["host"], "new.example.com");
        assert_eq!(merged["password"], "secret");
        assert_eq!(merged["url"], "https://hooks.example.com/secret");
        assert_eq!(merged["api_base_url"], "https://new-api.example.com");
    }

    #[test]
    fn sensitive_key_catalog_is_stable() {
        let keys = SENSITIVE_CONFIG_KEYS
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        assert_eq!(keys.len(), SENSITIVE_CONFIG_KEYS.len());
    }

    #[test]
    fn masks_email_and_opaque_targets() {
        assert_eq!(mask_target("alice@example.com"), "a***@example.com");
        assert_eq!(mask_target("U0123456"), "U0***56");
        assert_eq!(mask_target("ops"), "***");
    }
}
