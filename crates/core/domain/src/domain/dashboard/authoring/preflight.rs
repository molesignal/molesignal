// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::DashboardAuthoringSpec;
use crate::shared::{Result, contracts::ContractIssue, ids::Id};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PreflightStatus {
    Passed,
    Empty,
    Skipped,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PanelPreflight {
    pub path: String,
    pub title: String,
    pub query_kind: String,
    pub status: PreflightStatus,
    pub tested_from_micros: i64,
    pub tested_to_micros: i64,
    pub returned_rows: usize,
    pub scanned_rows: u64,
    pub took_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreflightWarning {
    pub code: String,
    pub path: String,
    pub message: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PreflightReport {
    pub panels: Vec<PanelPreflight>,
    pub warnings: Vec<PreflightWarning>,
    pub issues: Vec<ContractIssue>,
}

#[async_trait]
pub trait DashboardQueryPreflight: Send + Sync {
    async fn preflight(
        &self,
        org_id: &Id,
        actor: &Id,
        spec: &DashboardAuthoringSpec,
    ) -> Result<PreflightReport>;
}
