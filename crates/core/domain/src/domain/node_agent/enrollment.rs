// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::BTreeMap;

use kernel::{Error, Result, ids::Id, time::TimestampMicros};
use serde::Serialize;

/// A token that lets agents enroll into one organization. Only its hash is stored; the plaintext is
/// shown once, when it is issued.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnrollmentToken {
    pub id: Id,
    pub org_id: Id,
    pub description: String,
    /// Labels every node enrolled with this token gets.
    pub labels: BTreeMap<String, String>,
    pub max_uses: u32,
    pub uses: u32,
    pub expires_at: TimestampMicros,
    pub revoked_at: Option<TimestampMicros>,
    pub last_used_at: Option<TimestampMicros>,
    pub created_by: String,
    pub created_at: TimestampMicros,
}

impl EnrollmentToken {
    /// Longest validity of a token.
    pub const MAX_TTL_MICROS: i64 = 30 * 24 * 3600 * 1_000_000;
    pub const MAX_USES: u32 = 10_000;

    /// Why the token cannot enroll another node now, if it cannot. One message for every reason, so
    /// a caller learns nothing about tokens it does not hold.
    pub fn check_usable(&self, now: TimestampMicros) -> Result<()> {
        if self.revoked_at.is_some() || self.expires_at <= now || self.uses >= self.max_uses {
            return Err(Error::unauthorized("invalid or expired enrollment token"));
        }

        Ok(())
    }
}

/// A token as issued: the stored record and the plaintext the operator hands to the agent.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NewEnrollmentToken {
    #[serde(flatten)]
    pub token: EnrollmentToken,
    pub plaintext: String,
}

/// What a valid token grants: the organization and the labels of the node.
#[derive(Debug, Clone)]
pub struct EnrollmentGrant {
    pub token_id: Id,
    pub org_id: Id,
    pub labels: BTreeMap<String, String>,
}
