// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Bounded management queries for growing Status Page datasets.

use serde::{Deserialize, Serialize};

use super::{PublicIncidentStatus, StatusPageIncident, StatusPageIncidentKind};
use crate::shared::{ids::Id, time::TimestampMicros};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusPageHistoryQuery {
    pub kind: Option<StatusPageIncidentKind>,
    pub status: Option<PublicIncidentStatus>,
    pub component_id: Option<Id>,
    pub from: Option<TimestampMicros>,
    pub to: Option<TimestampMicros>,
    pub search: Option<String>,
    pub page: u32,
    pub per_page: u32,
}

impl Default for StatusPageHistoryQuery {
    fn default() -> Self {
        Self {
            kind: None,
            status: None,
            component_id: None,
            from: None,
            to: None,
            search: None,
            page: 1,
            per_page: 25,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusPageHistoryPage {
    pub items: Vec<StatusPageIncident>,
    pub page: u32,
    pub per_page: u32,
    pub total: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatusPageEventView {
    Current,
    Draft,
    Resolved,
    Upcoming,
    InProgress,
    Completed,
}

impl StatusPageEventView {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "current" => Some(Self::Current),
            "draft" => Some(Self::Draft),
            "resolved" => Some(Self::Resolved),
            "upcoming" => Some(Self::Upcoming),
            "in_progress" | "in-progress" => Some(Self::InProgress),
            "completed" => Some(Self::Completed),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusPageEventList {
    pub kind: StatusPageIncidentKind,
    pub view: StatusPageEventView,
    pub items: Vec<StatusPageIncident>,
    pub total: u64,
}
