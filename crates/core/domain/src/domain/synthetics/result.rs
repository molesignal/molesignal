// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::monitor::AssertionSeverity;
use crate::shared::{ids::Id, time::TimestampMicros};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbeOutcome {
    Healthy,
    Flaky,
    Degraded,
    Failing,
    Unknown,
    Skipped,
}

impl ProbeOutcome {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Healthy => "healthy",
            Self::Flaky => "flaky",
            Self::Degraded => "degraded",
            Self::Failing => "failing",
            Self::Unknown => "unknown",
            Self::Skipped => "skipped",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "healthy" => Some(Self::Healthy),
            "flaky" => Some(Self::Flaky),
            "degraded" => Some(Self::Degraded),
            "failing" => Some(Self::Failing),
            "unknown" => Some(Self::Unknown),
            "skipped" => Some(Self::Skipped),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TimingBreakdown {
    pub dns_micros: Option<u64>,
    pub connect_micros: Option<u64>,
    pub tls_micros: Option<u64>,
    pub first_byte_micros: Option<u64>,
    pub total_micros: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeAttempt {
    pub number: u32,
    pub started_at: TimestampMicros,
    pub finished_at: TimestampMicros,
    pub outcome: ProbeOutcome,
    pub timing: TimingBreakdown,
    pub error_category: Option<String>,
    pub error_message: Option<String>,
    pub bounded_response_excerpt: Option<Vec<u8>>,
    pub metadata: BTreeMap<String, String>,
    #[serde(default)]
    pub evidence: Vec<ProbeStepEvidence>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeStepEvidence {
    pub step_id: Id,
    pub name: String,
    pub action: String,
    pub started_at: TimestampMicros,
    pub finished_at: TimestampMicros,
    pub outcome: ProbeOutcome,
    pub error_category: Option<String>,
    pub error_message: Option<String>,
    pub metadata: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssertionObservation {
    pub assertion_id: Id,
    pub severity: AssertionSeverity,
    pub passed: bool,
    pub actual: Option<String>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyntheticResultArtifact {
    pub id: Id,
    pub name: String,
    pub kind: String,
    #[serde(skip, default)]
    pub object_key: String,
    pub content_type: String,
    pub content_length: u64,
    pub sha256: String,
    pub expires_at: TimestampMicros,
    pub created_at: TimestampMicros,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyntheticResult {
    pub id: Id,
    pub organization_id: Id,
    pub monitor_id: Id,
    pub monitor_revision_id: Id,
    pub location_id: Id,
    pub agent_id: Option<Id>,
    pub task_id: Id,
    pub is_test: bool,
    pub result_sequence: Option<u64>,
    pub scheduled_at: TimestampMicros,
    pub started_at: TimestampMicros,
    pub finished_at: TimestampMicros,
    pub received_at: TimestampMicros,
    pub outcome: ProbeOutcome,
    pub attempts: Vec<ProbeAttempt>,
    pub assertions: Vec<AssertionObservation>,
    #[serde(default)]
    pub artifacts: Vec<SyntheticResultArtifact>,
    pub secret_versions: BTreeMap<String, u32>,
    pub protocol_version: u32,
    pub metadata: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default)]
pub struct SyntheticResultListQuery {
    pub check_query: Option<String>,
    pub outcome: Option<ProbeOutcome>,
    pub location_id: Option<Id>,
    pub offset: u64,
    pub limit: u32,
}

#[derive(Debug, Clone)]
pub struct SyntheticResultPage {
    pub items: Vec<SyntheticResult>,
    pub total: u64,
}
