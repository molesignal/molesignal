// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde::{Deserialize, Serialize};

use crate::shared::{ids::Id, time::TimestampMicros};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeRegisterToken {
    pub id: Id,
    pub organization_id: Id,
    pub location_id: Id,
    pub expires_at: TimestampMicros,
    pub created_by: Id,
    pub created_at: TimestampMicros,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProbeRegisterInstructions {
    pub token: ProbeRegisterToken,
    pub register_token: String,
    pub register_endpoint: String,
    pub control_endpoint: String,
    pub ca_certificate_pem: String,
    pub ca_sha256: String,
    pub command: String,
}
