// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{
    domain::{alerting::incident::Severity, status_page::IncidentImpact},
    shared::{ids::Id, time::TimestampMicros},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationSourceKind {
    AlertIncident,
    SyntheticMonitor,
}

impl AutomationSourceKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AlertIncident => "alert_incident",
            Self::SyntheticMonitor => "synthetic_monitor",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "alert_incident" => Some(Self::AlertIncident),
            "synthetic_monitor" => Some(Self::SyntheticMonitor),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationRuleLifecycle {
    Draft,
    Active,
    Paused,
    Archived,
}

impl AutomationRuleLifecycle {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Active => "active",
            Self::Paused => "paused",
            Self::Archived => "archived",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "draft" => Some(Self::Draft),
            "active" => Some(Self::Active),
            "paused" => Some(Self::Paused),
            "archived" => Some(Self::Archived),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationPublicationMode {
    Approval,
    Automatic,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationMatchers {
    pub source_kind: AutomationSourceKind,
    pub source_id: Option<Id>,
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
    pub minimum_severity: Option<Severity>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationImpactMap {
    pub degraded: IncidentImpact,
    pub failing: IncidentImpact,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationTemplates {
    pub title: String,
    pub investigating: String,
    pub update: String,
    pub resolved: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationAction {
    pub component_ids: Vec<Id>,
    pub publication_mode: AutomationPublicationMode,
    pub sustained_delay_seconds: u32,
    pub impact_map: AutomationImpactMap,
    pub templates: AutomationTemplates,
    pub correlation_key_template: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusAutomationRule {
    pub id: Id,
    pub organization_id: Id,
    pub status_page_id: Id,
    pub name: String,
    pub position: i32,
    pub lifecycle: AutomationRuleLifecycle,
    pub active_revision_id: Option<Id>,
    pub draft_revision_id: Option<Id>,
    pub created_by: Id,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusAutomationRevision {
    pub id: Id,
    pub organization_id: Id,
    pub status_page_id: Id,
    pub rule_id: Id,
    pub number: u32,
    pub matchers: AutomationMatchers,
    pub action: AutomationAction,
    pub content_hash: String,
    pub created_by: Id,
    pub created_at: TimestampMicros,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveAutomationRule {
    pub rule: StatusAutomationRule,
    pub revision: StatusAutomationRevision,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationCandidateState {
    Delayed,
    PendingApproval,
    Approved,
    Published,
    Rejected,
    Cancelled,
    Resolved,
    Failed,
}

impl AutomationCandidateState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Delayed => "delayed",
            Self::PendingApproval => "pending_approval",
            Self::Approved => "approved",
            Self::Published => "published",
            Self::Rejected => "rejected",
            Self::Cancelled => "cancelled",
            Self::Resolved => "resolved",
            Self::Failed => "failed",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "delayed" => Some(Self::Delayed),
            "pending_approval" => Some(Self::PendingApproval),
            "approved" => Some(Self::Approved),
            "published" => Some(Self::Published),
            "rejected" => Some(Self::Rejected),
            "cancelled" => Some(Self::Cancelled),
            "resolved" => Some(Self::Resolved),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationCandidate {
    pub id: Id,
    pub organization_id: Id,
    pub status_page_id: Id,
    pub rule_revision_id: Id,
    pub correlation_key: String,
    pub state: AutomationCandidateState,
    pub title: String,
    pub message: String,
    pub resolved_message: String,
    pub impact: IncidentImpact,
    pub component_ids: Vec<Id>,
    pub automatic: bool,
    pub status_incident_id: Option<Id>,
    pub due_at: TimestampMicros,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationSourceObservation {
    pub organization_id: Id,
    pub source_kind: AutomationSourceKind,
    pub source_id: Id,
    pub source_instance_id: Id,
    pub severity: Severity,
    pub labels: BTreeMap<String, String>,
    pub active: bool,
    pub muted: bool,
    pub observed_at: TimestampMicros,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationCandidateSource {
    pub source_kind: AutomationSourceKind,
    pub source_id: Id,
    pub source_instance_id: Id,
    pub severity: Severity,
    pub labels: BTreeMap<String, String>,
    pub active: bool,
    pub muted: bool,
    pub observed_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationCandidateAction {
    pub id: Id,
    pub action: String,
    pub actor_id: Option<Id>,
    pub note: Option<String>,
    pub created_at: TimestampMicros,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationWorkItem {
    pub id: Id,
    pub kind: String,
    pub status: String,
    pub attempts: u32,
    pub available_at: TimestampMicros,
    pub last_error: Option<String>,
    pub completed_at: Option<TimestampMicros>,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationCandidateDetail {
    pub candidate: AutomationCandidate,
    pub sources: Vec<AutomationCandidateSource>,
    pub actions: Vec<AutomationCandidateAction>,
    pub work_items: Vec<AutomationWorkItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationOutboxItem {
    pub id: Id,
    pub organization_id: Id,
    pub candidate_id: Id,
    pub kind: String,
    pub idempotency_key: String,
    pub payload: serde_json::Value,
    pub attempts: u32,
    pub available_at: TimestampMicros,
    pub created_at: TimestampMicros,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationSettings {
    pub organization_id: Id,
    pub status_page_id: Id,
    pub paused: bool,
    pub updated_by: Id,
    pub updated_at: TimestampMicros,
}
