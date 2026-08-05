// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Database-backed IAM permission catalog contracts.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IamPermissionScope {
    Platform,
    Organization,
}

impl IamPermissionScope {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Platform => "platform",
            Self::Organization => "organization",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IamPermissionDefinition {
    pub key: String,
    pub scope: IamPermissionScope,
    pub domain: String,
    pub label_key: String,
    pub description_key: String,
    #[serde(default)]
    pub builtin_roles: Vec<String>,
    #[serde(default)]
    pub feature: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IamPermissionBundle {
    pub key: String,
    pub label_key: String,
    pub description_key: String,
    pub permissions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IamPermissionCatalog {
    pub version: u64,
    pub permissions: Vec<IamPermissionDefinition>,
    #[serde(default)]
    pub bundles: Vec<IamPermissionBundle>,
}
