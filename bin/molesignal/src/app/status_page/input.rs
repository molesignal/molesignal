// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Inputs accepted by status-page use cases.

use crate::{
    domain::status_page::{
        ComponentLifecycle, ComponentStatus, ComponentVisibility, IncidentImpact,
        PublicIncidentStatus, StatusPageAccessRuleKind, StatusPageIncidentKind,
        StatusPagePublicationState, StatusPageVisibility,
    },
    shared::{ids::Id, time::TimestampMicros},
};

#[derive(Debug, Clone)]
pub struct StatusPageInput {
    pub name: String,
    pub slug: String,
    pub logo_url: Option<String>,
    pub brand_color: String,
    pub custom_domain: Option<String>,
    pub timezone: String,
    pub language: String,
    pub languages: Vec<String>,
    pub history_days: i32,
    pub delivery_retention_days: i32,
    pub private_session_days: i32,
    pub visibility: StatusPageVisibility,
}

#[derive(Debug, Clone)]
pub struct StatusPageComponentInput {
    pub name: String,
    pub description: String,
    pub status: ComponentStatus,
    pub visibility: ComponentVisibility,
    pub lifecycle: ComponentLifecycle,
    pub position: Option<i32>,
}

#[derive(Debug, Clone)]
pub struct StatusPageIncidentInput {
    pub source_incident_id: Option<Id>,
    pub kind: StatusPageIncidentKind,
    pub title: String,
    pub impact: IncidentImpact,
    pub status: PublicIncidentStatus,
    pub publication_state: StatusPagePublicationState,
    pub message: Option<String>,
    pub component_ids: Vec<Id>,
    pub started_at: Option<TimestampMicros>,
}

#[derive(Debug, Clone)]
pub struct StatusPageDomainInput {
    pub hostname: String,
}

#[derive(Debug, Clone)]
pub struct StatusPageAccessRuleInput {
    pub kind: StatusPageAccessRuleKind,
    pub value: String,
}

#[derive(Debug, Clone)]
pub struct StatusPageIncidentUpdateInput {
    pub status: PublicIncidentStatus,
    pub message: String,
}
