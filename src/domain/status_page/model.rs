// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde::{Deserialize, Serialize};

use crate::shared::{ids::Id, time::TimestampMicros};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatusPageVisibility {
    Public,
    Private,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatusPageLifecycle {
    Active,
    Archived,
}

impl StatusPageLifecycle {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Archived => "archived",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "active" => Some(Self::Active),
            "archived" => Some(Self::Archived),
            _ => None,
        }
    }
}

impl StatusPageVisibility {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Private => "private",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "public" => Some(Self::Public),
            "private" => Some(Self::Private),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComponentStatus {
    Operational,
    DegradedPerformance,
    PartialOutage,
    MajorOutage,
    Maintenance,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComponentVisibility {
    Enabled,
    Hidden,
}

impl ComponentVisibility {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Enabled => "enabled",
            Self::Hidden => "hidden",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "enabled" => Some(Self::Enabled),
            "hidden" => Some(Self::Hidden),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComponentLifecycle {
    Active,
    Archived,
}

impl ComponentLifecycle {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Archived => "archived",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "active" => Some(Self::Active),
            "archived" => Some(Self::Archived),
            _ => None,
        }
    }
}

impl ComponentStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Operational => "operational",
            Self::DegradedPerformance => "degraded_performance",
            Self::PartialOutage => "partial_outage",
            Self::MajorOutage => "major_outage",
            Self::Maintenance => "maintenance",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "operational" => Some(Self::Operational),
            "degraded_performance" => Some(Self::DegradedPerformance),
            "partial_outage" => Some(Self::PartialOutage),
            "major_outage" => Some(Self::MajorOutage),
            "maintenance" => Some(Self::Maintenance),
            _ => None,
        }
    }

    pub const fn severity_rank(self) -> u8 {
        match self {
            Self::Operational => 0,
            Self::Maintenance => 1,
            Self::DegradedPerformance => 2,
            Self::PartialOutage => 3,
            Self::MajorOutage => 4,
        }
    }

    pub fn worst(left: Self, right: Self) -> Self {
        if right.severity_rank() > left.severity_rank() {
            right
        } else {
            left
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatusPageIncidentKind {
    Incident,
    Maintenance,
}

impl StatusPageIncidentKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Incident => "incident",
            Self::Maintenance => "maintenance",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "incident" => Some(Self::Incident),
            "maintenance" => Some(Self::Maintenance),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IncidentImpact {
    Minor,
    Major,
    Critical,
    Maintenance,
}

impl IncidentImpact {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Minor => "minor",
            Self::Major => "major",
            Self::Critical => "critical",
            Self::Maintenance => "maintenance",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "minor" => Some(Self::Minor),
            "major" => Some(Self::Major),
            "critical" => Some(Self::Critical),
            "maintenance" => Some(Self::Maintenance),
            _ => None,
        }
    }

    pub const fn component_status(self) -> ComponentStatus {
        match self {
            Self::Minor => ComponentStatus::DegradedPerformance,
            Self::Major => ComponentStatus::PartialOutage,
            Self::Critical => ComponentStatus::MajorOutage,
            Self::Maintenance => ComponentStatus::Maintenance,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PublicIncidentStatus {
    Investigating,
    Identified,
    InProgress,
    Monitoring,
    Resolved,
    Scheduled,
    Completed,
    Cancelled,
}

impl PublicIncidentStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Investigating => "investigating",
            Self::Identified => "identified",
            Self::InProgress => "in_progress",
            Self::Monitoring => "monitoring",
            Self::Resolved => "resolved",
            Self::Scheduled => "scheduled",
            Self::Completed => "completed",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "investigating" => Some(Self::Investigating),
            "identified" => Some(Self::Identified),
            "in_progress" => Some(Self::InProgress),
            "monitoring" => Some(Self::Monitoring),
            "resolved" => Some(Self::Resolved),
            "scheduled" => Some(Self::Scheduled),
            "completed" => Some(Self::Completed),
            "cancelled" => Some(Self::Cancelled),
            _ => None,
        }
    }

    pub const fn is_resolved(self) -> bool {
        matches!(self, Self::Resolved)
    }

    pub const fn is_terminal_for(self, kind: StatusPageIncidentKind) -> bool {
        match kind {
            StatusPageIncidentKind::Incident => matches!(self, Self::Resolved),
            StatusPageIncidentKind::Maintenance => {
                matches!(self, Self::Completed | Self::Cancelled)
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatusPagePublicationState {
    Draft,
    Published,
}

impl StatusPagePublicationState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Published => "published",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "draft" => Some(Self::Draft),
            "published" => Some(Self::Published),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusPage {
    pub id: Id,
    pub org_id: Id,
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
    pub lifecycle: StatusPageLifecycle,
    pub archived_at: Option<TimestampMicros>,
    pub purge_after: Option<TimestampMicros>,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusPageComponent {
    pub id: Id,
    pub org_id: Id,
    pub status_page_id: Id,
    pub name: String,
    pub description: String,
    pub status: ComponentStatus,
    pub visibility: ComponentVisibility,
    pub lifecycle: ComponentLifecycle,
    pub position: i32,
    pub archived_at: Option<TimestampMicros>,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusPageComponentStatusEvent {
    pub id: Id,
    pub org_id: Id,
    pub status_page_id: Id,
    pub component_id: Id,
    pub status: ComponentStatus,
    pub started_at: TimestampMicros,
    pub ended_at: Option<TimestampMicros>,
    pub created_at: TimestampMicros,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusPageIncidentUpdate {
    pub id: Id,
    pub org_id: Id,
    pub status_page_id: Id,
    pub incident_id: Id,
    pub status: PublicIncidentStatus,
    pub message: String,
    pub created_at: TimestampMicros,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusPageIncident {
    pub id: Id,
    pub org_id: Id,
    pub status_page_id: Id,
    pub source_incident_id: Option<Id>,
    pub kind: StatusPageIncidentKind,
    pub title: String,
    pub impact: IncidentImpact,
    pub status: PublicIncidentStatus,
    pub publication_state: StatusPagePublicationState,
    pub draft_message: Option<String>,
    pub component_ids: Vec<Id>,
    pub updates: Vec<StatusPageIncidentUpdate>,
    pub started_at: TimestampMicros,
    pub ended_at: Option<TimestampMicros>,
    pub published_at: Option<TimestampMicros>,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusPageSnapshot {
    pub page: StatusPage,
    pub overall_status: ComponentStatus,
    pub components: Vec<StatusPageComponent>,
    pub component_status_events: Vec<StatusPageComponentStatusEvent>,
    pub active_incidents: Vec<StatusPageIncident>,
    pub scheduled_maintenance: Vec<StatusPageIncident>,
    pub history: Vec<StatusPageIncident>,
    /// Most recent persisted business change included in this snapshot.
    pub updated_at: TimestampMicros,
    /// Request-time boundary used to close open history intervals.
    pub generated_at: TimestampMicros,
}

impl StatusPageSnapshot {
    pub fn build(
        page: StatusPage,
        mut components: Vec<StatusPageComponent>,
        component_status_events: Vec<StatusPageComponentStatusEvent>,
        incidents: Vec<StatusPageIncident>,
        generated_at: TimestampMicros,
    ) -> Self {
        let updated_at = components
            .iter()
            .map(|component| component.updated_at)
            .chain(incidents.iter().map(|incident| incident.updated_at))
            .chain(
                incidents
                    .iter()
                    .flat_map(|incident| incident.updates.iter().map(|update| update.created_at)),
            )
            .chain(
                component_status_events
                    .iter()
                    .map(|event| event.ended_at.unwrap_or(event.created_at)),
            )
            .fold(page.updated_at, std::cmp::max);
        components.sort_by_key(|component| (component.position, component.name.clone()));
        let mut overall_status = components
            .iter()
            .filter(|component| {
                component.visibility == ComponentVisibility::Enabled
                    && component.lifecycle == ComponentLifecycle::Active
            })
            .fold(ComponentStatus::Operational, |current, component| {
                ComponentStatus::worst(current, component.status)
            });
        let mut active_incidents = Vec::new();
        let mut scheduled_maintenance = Vec::new();
        let mut history = Vec::new();

        for incident in incidents {
            if incident.publication_state == StatusPagePublicationState::Draft {
                continue;
            }
            if incident.status.is_terminal_for(incident.kind) {
                history.push(incident);
            } else if incident.kind == StatusPageIncidentKind::Maintenance
                && (incident.status == PublicIncidentStatus::Scheduled
                    || incident.started_at > generated_at)
            {
                scheduled_maintenance.push(incident);
            } else {
                let impact_status = incident.impact.component_status();
                if components.iter().any(|component| {
                    component.visibility == ComponentVisibility::Enabled
                        && component.lifecycle == ComponentLifecycle::Active
                        && incident.component_ids.contains(&component.id)
                }) {
                    overall_status = ComponentStatus::worst(overall_status, impact_status);
                }
                active_incidents.push(incident);
            }
        }
        active_incidents.sort_by_key(|incident| std::cmp::Reverse(incident.started_at));
        scheduled_maintenance.sort_by_key(|incident| incident.started_at);
        history.sort_by_key(|incident| {
            std::cmp::Reverse(incident.ended_at.unwrap_or(incident.started_at))
        });

        Self {
            page,
            overall_status,
            components,
            component_status_events,
            active_incidents,
            scheduled_maintenance,
            history,
            updated_at,
            generated_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page() -> StatusPage {
        StatusPage {
            id: Id("page".into()),
            org_id: Id("org".into()),
            name: "Acme".into(),
            slug: "acme".into(),
            logo_url: None,
            brand_color: "#2563EB".into(),
            custom_domain: None,
            timezone: "UTC".into(),
            language: "en-us".into(),
            languages: vec!["en-us".into()],
            history_days: 90,
            delivery_retention_days: 90,
            private_session_days: 7,
            visibility: StatusPageVisibility::Public,
            lifecycle: StatusPageLifecycle::Active,
            archived_at: None,
            purge_after: None,
            created_at: TimestampMicros(1),
            updated_at: TimestampMicros(1),
        }
    }

    fn incident(
        kind: StatusPageIncidentKind,
        status: PublicIncidentStatus,
        at: i64,
    ) -> StatusPageIncident {
        StatusPageIncident {
            id: Id(format!("incident-{at}")),
            org_id: Id("org".into()),
            status_page_id: Id("page".into()),
            source_incident_id: None,
            kind,
            title: "Issue".into(),
            impact: if kind == StatusPageIncidentKind::Maintenance {
                IncidentImpact::Maintenance
            } else {
                IncidentImpact::Critical
            },
            status,
            publication_state: StatusPagePublicationState::Published,
            draft_message: None,
            component_ids: vec![Id("component".into())],
            updates: Vec::new(),
            started_at: TimestampMicros(at),
            ended_at: status
                .is_terminal_for(kind)
                .then_some(TimestampMicros(at + 1)),
            published_at: Some(TimestampMicros(at)),
            created_at: TimestampMicros(at),
            updated_at: TimestampMicros(at),
        }
    }

    #[test]
    fn snapshot_separates_future_maintenance_and_history() {
        let snapshot = StatusPageSnapshot::build(
            page(),
            vec![StatusPageComponent {
                id: Id("component".into()),
                org_id: Id("org".into()),
                status_page_id: Id("page".into()),
                name: "API".into(),
                description: String::new(),
                status: ComponentStatus::Operational,
                visibility: ComponentVisibility::Enabled,
                lifecycle: ComponentLifecycle::Active,
                position: 0,
                archived_at: None,
                created_at: TimestampMicros(1),
                updated_at: TimestampMicros(1),
            }],
            Vec::new(),
            vec![
                incident(
                    StatusPageIncidentKind::Incident,
                    PublicIncidentStatus::Investigating,
                    5,
                ),
                incident(
                    StatusPageIncidentKind::Maintenance,
                    PublicIncidentStatus::Scheduled,
                    20,
                ),
                incident(
                    StatusPageIncidentKind::Incident,
                    PublicIncidentStatus::Resolved,
                    2,
                ),
            ],
            TimestampMicros(10),
        );

        assert_eq!(snapshot.active_incidents.len(), 1);
        assert_eq!(snapshot.scheduled_maintenance.len(), 1);
        assert_eq!(snapshot.history.len(), 1);
        assert_eq!(snapshot.overall_status, ComponentStatus::MajorOutage);
        assert_eq!(snapshot.updated_at, TimestampMicros(20));
    }
}
