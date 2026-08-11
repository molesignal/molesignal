// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde::{Deserialize, Serialize};

use crate::shared::{ids::Id, time::TimestampMicros};

#[derive(Clone)]
pub struct SecretMaterial(pub Vec<u8>);

impl std::fmt::Debug for SecretMaterial {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("SecretMaterial([REDACTED])")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyntheticSecret {
    pub id: Id,
    pub organization_id: Id,
    pub name: String,
    pub description: String,
    pub current_version: u32,
    pub created_by: Id,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
    pub archived_at: Option<TimestampMicros>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyntheticSecretVersion {
    pub secret_id: Id,
    pub organization_id: Id,
    pub version: u32,
    pub created_by: Id,
    pub created_at: TimestampMicros,
}
