// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Immutable Dashboard contract publications and the global authoring binding.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::shared::{Result, time::TimestampMicros};

pub const DASHBOARD_AUTHORING_CAPABILITY: &str = "dashboard.authoring.v1";
pub const DASHBOARD_MODEL_CONTRACT: &str = "dashboard.model";
pub const DASHBOARD_AUTHORING_CONTRACT: &str = "dashboard.authoring";
pub const DASHBOARD_VISUALIZATION_CONTRACT: &str = "dashboard.visualizations";
pub const JSON_SCHEMA_2020_12_DIALECT: &str = "https://json-schema.org/draft/2020-12/schema";
pub const VISUALIZATION_MANIFEST_DIALECT: &str = "molesignal.visualization-manifest/v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DashboardContractKind {
    DashboardModel,
    DashboardAuthoring,
    VisualizationManifest,
}

impl DashboardContractKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DashboardModel => "dashboard_model",
            Self::DashboardAuthoring => "dashboard_authoring",
            Self::VisualizationManifest => "visualization_manifest",
        }
    }

    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "dashboard_model" => Some(Self::DashboardModel),
            "dashboard_authoring" => Some(Self::DashboardAuthoring),
            "visualization_manifest" => Some(Self::VisualizationManifest),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DashboardContractStatus {
    Published,
    Disabled,
}

impl DashboardContractStatus {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Published => "published",
            Self::Disabled => "disabled",
        }
    }

    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "published" => Some(Self::Published),
            "disabled" => Some(Self::Disabled),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DashboardContractRef {
    pub contract_key: String,
    pub version: u32,
    pub schema_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardContractVersion {
    pub contract_key: String,
    pub version: u32,
    pub kind: DashboardContractKind,
    pub dialect: String,
    pub document: Value,
    pub schema_hash: String,
    pub status: DashboardContractStatus,
    pub published_at: TimestampMicros,
}

impl DashboardContractVersion {
    #[must_use]
    pub fn reference(&self) -> DashboardContractRef {
        DashboardContractRef {
            contract_key: self.contract_key.clone(),
            version: self.version,
            schema_hash: self.schema_hash.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DashboardContractSelection {
    pub capability_key: String,
    pub model: DashboardContractRef,
    pub authoring: DashboardContractRef,
    pub visualization: DashboardContractRef,
    pub compiler_version: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DashboardContractBinding {
    pub selection: DashboardContractSelection,
    pub revision: i64,
    pub updated_at: TimestampMicros,
}

#[derive(Debug, Clone)]
pub struct DashboardContractDocuments {
    pub model: DashboardContractVersion,
    pub authoring: DashboardContractVersion,
    pub visualization: DashboardContractVersion,
}

#[derive(Debug, Clone)]
pub struct DashboardContractBundle {
    pub binding: DashboardContractBinding,
    pub documents: DashboardContractDocuments,
}

#[async_trait]
pub trait DashboardContractRepository: Send + Sync {
    /// Inserts immutable built-in versions and creates the default binding only when absent.
    async fn publish_builtin(
        &self,
        versions: &[DashboardContractVersion],
        default_selection: &DashboardContractSelection,
        now: TimestampMicros,
    ) -> Result<DashboardContractBinding>;

    async fn load_active(&self, capability_key: &str) -> Result<DashboardContractBundle>;

    async fn load_documents(
        &self,
        selection: &DashboardContractSelection,
    ) -> Result<DashboardContractDocuments>;

    /// Trusted internal activation atomically updates every reference and increments revision.
    async fn activate(
        &self,
        selection: &DashboardContractSelection,
        now: TimestampMicros,
    ) -> Result<DashboardContractBinding>;
}
