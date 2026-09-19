// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde::{Deserialize, Serialize};

pub const DEFAULT_SERVICE_NAMESPACE: &str = "default";
pub const DEFAULT_SERVICE_NAME: &str = "unknown_service";
pub const DEFAULT_DEPLOYMENT_ENVIRONMENT: &str = "unknown";
pub const OTHER_DIMENSION: &str = "__other__";

fn normalized_or(value: Option<&str>, fallback: &str) -> String {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(fallback)
        .to_owned()
}

/// Stable service identity. Version and instance deliberately do not belong
/// to this key.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ServiceIdentity {
    pub namespace: String,
    pub name: String,
    pub environment: String,
}

impl ServiceIdentity {
    pub fn new(
        namespace: Option<&str>,
        name: Option<&str>,
        deployment_environment_name: Option<&str>,
        legacy_deployment_environment: Option<&str>,
    ) -> Self {
        let environment = deployment_environment_name
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .or_else(|| {
                legacy_deployment_environment
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
            });
        Self {
            namespace: normalized_or(namespace, DEFAULT_SERVICE_NAMESPACE),
            name: normalized_or(name, DEFAULT_SERVICE_NAME),
            environment: normalized_or(environment, DEFAULT_DEPLOYMENT_ENVIRONMENT),
        }
    }

    pub fn stable_key(&self) -> String {
        format!("{}/{}/{}", self.namespace, self.name, self.environment)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransactionKind {
    Http,
    Rpc,
    Messaging,
    Span,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TransactionIdentity {
    pub name: String,
    pub kind: TransactionKind,
}

impl TransactionIdentity {
    pub fn other() -> Self {
        Self {
            name: OTHER_DIMENSION.to_owned(),
            kind: TransactionKind::Other,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DependencyCategory {
    Service,
    Database,
    Cache,
    Messaging,
    ExternalHttp,
    ExternalRpc,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct DependencyIdentity {
    pub category: DependencyCategory,
    pub target: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation: Option<String>,
}

impl DependencyIdentity {
    pub fn other(category: DependencyCategory) -> Self {
        Self {
            category,
            target: OTHER_DIMENSION.to_owned(),
            operation: None,
        }
    }
}

/// Stable backend error grouping identity. Representative messages are not
/// part of this identity because request-specific values would split groups.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ErrorIdentity {
    pub fingerprint: String,
    pub error_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub application_frame: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transaction_name: Option<String>,
    #[serde(default)]
    pub overflow: bool,
}
