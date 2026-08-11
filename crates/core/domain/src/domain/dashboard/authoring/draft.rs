// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{PreflightReport, VisualizationCapability};
use crate::{
    domain::dashboard::Dashboard,
    shared::{contracts::ContractIssue, ids::Id, time::TimestampMicros},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DashboardDraftStatus {
    Ready,
    Consumed,
    Expired,
}

impl DashboardDraftStatus {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Consumed => "consumed",
            Self::Expired => "expired",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "ready" => Some(Self::Ready),
            "consumed" => Some(Self::Consumed),
            "expired" => Some(Self::Expired),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardDraft {
    pub id: Id,
    pub org_id: Id,
    pub created_by: Id,
    pub authoring_version: u32,
    pub model_schema_version: u32,
    pub compiler_version: String,
    pub contract_binding_revision: i64,
    pub authoring_schema_hash: String,
    pub model_schema_hash: String,
    pub visualization_schema_hash: String,
    pub authoring_spec: Value,
    pub compiled_model: Value,
    pub model_hash: String,
    pub folder_id: Option<Id>,
    pub status: DashboardDraftStatus,
    pub dashboard_id: Option<Id>,
    pub warnings: Vec<PreflightWarningRecord>,
    pub preflight: PreflightReport,
    pub created_at: TimestampMicros,
    pub expires_at: TimestampMicros,
    pub consumed_at: Option<TimestampMicros>,
}

impl DashboardDraft {
    #[must_use]
    pub fn is_expired_at(&self, now: TimestampMicros) -> bool {
        self.status == DashboardDraftStatus::Expired
            || (self.status == DashboardDraftStatus::Ready && self.expires_at <= now)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreflightWarningRecord {
    pub code: String,
    pub path: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardAuthoringCapabilities {
    pub authoring_versions: Vec<u32>,
    pub dashboard_model_version: u32,
    pub compiler_version: String,
    pub query_kinds: Vec<String>,
    pub visualizations: Vec<VisualizationCapability>,
    pub units: Vec<String>,
    pub reducers: Vec<String>,
    pub limits: Value,
    pub workflow: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreparedDashboardDraft {
    pub draft_id: Id,
    pub model_hash: String,
    pub expires_at: TimestampMicros,
    pub summary: Value,
    pub warnings: Vec<PreflightWarningRecord>,
    pub issues: Vec<ContractIssue>,
    pub preview_route: String,
}

#[derive(Debug, Clone)]
pub enum DraftConsumption {
    Created(Dashboard),
    Replay(Dashboard),
}

impl DraftConsumption {
    #[must_use]
    pub fn dashboard(&self) -> &Dashboard {
        match self {
            Self::Created(dashboard) | Self::Replay(dashboard) => dashboard,
        }
    }

    #[must_use]
    pub const fn replayed(&self) -> bool {
        matches!(self, Self::Replay(_))
    }
}
