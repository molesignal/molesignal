// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Inputs accepted by status-page use cases.

use crate::{
    domain::status_page::{
        ComponentDependency, ComponentKind, ComponentLifecycle, ComponentStatus,
        ComponentVisibility, DependencyMode, IncidentImpact, PublicIncidentStatus,
        StatusPageAccessRuleKind, StatusPageIncident, StatusPageIncidentKind,
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
    /// Whether a change of status tells subscribers.
    pub notify_subscribers: bool,
    /// A component or a group; fixed once the component is made.
    pub kind: ComponentKind,
    /// The group the component is in.
    pub group_id: Option<Id>,
    /// What the component depends on; `None` keeps what it has.
    pub dependencies: Option<Vec<ComponentDependency>>,
    /// How the health of its dependencies reaches it; `None` keeps the mode it has.
    pub dependency_mode: Option<DependencyMode>,
    /// Whether a status it follows tells subscribers; `None` keeps what it has.
    pub dependency_notify: Option<bool>,
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
    /// Planned maintenance moves to in progress by itself at its planned
    /// start. `None` takes the default for new maintenance.
    pub auto_start: Option<bool>,
    /// How long before the planned start subscribers are reminded, zero for no
    /// reminder. `None` takes the default for new maintenance.
    pub reminder_minutes: Option<i32>,
    /// By when the first published update promises the next one.
    pub next_update_at: Option<TimestampMicros>,
    /// Whether the first published update tells subscribers.
    pub notify_subscribers: bool,
}

impl StatusPageIncidentInput {
    /// An edit that does not name these two options leaves them as the
    /// maintenance has them.
    pub(super) fn keep_planned_options_of(&mut self, existing: &StatusPageIncident) {
        if existing.kind != StatusPageIncidentKind::Maintenance {
            return;
        }

        self.auto_start = self.auto_start.or(Some(existing.auto_start));
        self.reminder_minutes = self.reminder_minutes.or(Some(existing.reminder_minutes));
    }
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
    /// By when the publisher promises the next update.
    pub next_update_at: Option<TimestampMicros>,
    /// Whether the update tells subscribers. A correction of wording usually
    /// does not.
    pub notify_subscribers: bool,
}
