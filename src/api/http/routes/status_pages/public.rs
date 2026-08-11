// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Explicit public projection for anonymous status-page responses.

use serde::Serialize;

use crate::{
    domain::status_page::{
        ComponentStatus, IncidentImpact, PublicIncidentStatus, StatusPageComponent,
        StatusPageComponentStatusEvent, StatusPageIncident, StatusPageIncidentKind,
        StatusPageIncidentUpdate, StatusPageSnapshot, StatusPageVisibility,
    },
    shared::{ids::Id, time::TimestampMicros},
};

#[derive(Debug, Serialize)]
pub(super) struct PublicStatusPageSnapshot {
    page: PublicStatusPage,
    overall_status: ComponentStatus,
    components: Vec<PublicStatusPageComponent>,
    component_status_events: Vec<PublicStatusPageComponentStatusEvent>,
    active_incidents: Vec<PublicStatusPageIncident>,
    scheduled_maintenance: Vec<PublicStatusPageIncident>,
    history: Vec<PublicStatusPageIncident>,
    updated_at: TimestampMicros,
    generated_at: TimestampMicros,
}

#[derive(Debug, Serialize)]
struct PublicStatusPage {
    name: String,
    slug: String,
    logo_url: Option<String>,
    brand_color: String,
    timezone: String,
    language: String,
    languages: Vec<String>,
    history_days: i32,
    visibility: StatusPageVisibility,
}

#[derive(Debug, Serialize)]
struct PublicStatusPageComponent {
    id: Id,
    name: String,
    description: String,
    status: ComponentStatus,
}

#[derive(Debug, Serialize)]
struct PublicStatusPageComponentStatusEvent {
    component_id: Id,
    status: ComponentStatus,
    started_at: TimestampMicros,
    ended_at: Option<TimestampMicros>,
}

#[derive(Debug, Serialize)]
struct PublicStatusPageIncident {
    id: Id,
    kind: StatusPageIncidentKind,
    title: String,
    impact: IncidentImpact,
    status: PublicIncidentStatus,
    component_ids: Vec<Id>,
    updates: Vec<PublicStatusPageIncidentUpdate>,
    started_at: TimestampMicros,
    ended_at: Option<TimestampMicros>,
}

#[derive(Debug, Serialize)]
struct PublicStatusPageIncidentUpdate {
    id: Id,
    status: PublicIncidentStatus,
    message: String,
    created_at: TimestampMicros,
}

impl From<StatusPageSnapshot> for PublicStatusPageSnapshot {
    fn from(snapshot: StatusPageSnapshot) -> Self {
        let page = PublicStatusPage {
            name: snapshot.page.name,
            slug: snapshot.page.slug,
            logo_url: snapshot.page.logo_url,
            brand_color: snapshot.page.brand_color,
            timezone: snapshot.page.timezone,
            language: snapshot.page.language,
            languages: snapshot.page.languages,
            history_days: snapshot.page.history_days,
            visibility: snapshot.page.visibility,
        };
        let active_incidents = snapshot.active_incidents;
        let mut components: Vec<_> = snapshot
            .components
            .into_iter()
            .map(PublicStatusPageComponent::from)
            .collect();
        for incident in &active_incidents {
            let impact_status = incident.impact.component_status();
            for component in &mut components {
                if incident.component_ids.contains(&component.id) {
                    component.status = ComponentStatus::worst(component.status, impact_status);
                }
            }
        }
        Self {
            page,
            overall_status: snapshot.overall_status,
            components,
            component_status_events: snapshot
                .component_status_events
                .into_iter()
                .map(PublicStatusPageComponentStatusEvent::from)
                .collect(),
            active_incidents: project_incidents(active_incidents),
            scheduled_maintenance: project_incidents(snapshot.scheduled_maintenance),
            history: project_incidents(snapshot.history),
            updated_at: snapshot.updated_at,
            generated_at: snapshot.generated_at,
        }
    }
}

impl From<StatusPageComponentStatusEvent> for PublicStatusPageComponentStatusEvent {
    fn from(event: StatusPageComponentStatusEvent) -> Self {
        Self {
            component_id: event.component_id,
            status: event.status,
            started_at: event.started_at,
            ended_at: event.ended_at,
        }
    }
}

impl From<StatusPageComponent> for PublicStatusPageComponent {
    fn from(component: StatusPageComponent) -> Self {
        Self {
            id: component.id,
            name: component.name,
            description: component.description,
            status: component.status,
        }
    }
}

impl From<StatusPageIncident> for PublicStatusPageIncident {
    fn from(incident: StatusPageIncident) -> Self {
        Self {
            id: incident.id,
            kind: incident.kind,
            title: incident.title,
            impact: incident.impact,
            status: incident.status,
            component_ids: incident.component_ids,
            updates: incident
                .updates
                .into_iter()
                .map(PublicStatusPageIncidentUpdate::from)
                .collect(),
            started_at: incident.started_at,
            ended_at: incident.ended_at,
        }
    }
}

impl From<StatusPageIncidentUpdate> for PublicStatusPageIncidentUpdate {
    fn from(update: StatusPageIncidentUpdate) -> Self {
        Self {
            id: update.id,
            status: update.status,
            message: update.message,
            created_at: update.created_at,
        }
    }
}

fn project_incidents(incidents: Vec<StatusPageIncident>) -> Vec<PublicStatusPageIncident> {
    incidents
        .into_iter()
        .map(PublicStatusPageIncident::from)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::status_page::{
        ComponentLifecycle, ComponentVisibility, StatusPage, StatusPageLifecycle,
        StatusPagePublicationState, StatusPageVisibility,
    };

    #[test]
    fn anonymous_projection_omits_tenant_and_internal_incident_identifiers() {
        let component_id = Id("component-public".into());
        let snapshot = StatusPageSnapshot {
            page: StatusPage {
                id: Id("page-secret".into()),
                org_id: Id("org-secret".into()),
                name: "Acme".into(),
                slug: "acme".into(),
                logo_url: None,
                brand_color: "#4F46E5".into(),
                custom_domain: Some("status.acme.test".into()),
                timezone: "UTC".into(),
                language: "en-us".into(),
                languages: vec!["en-us".into(), "zh-cn".into()],
                history_days: 30,
                delivery_retention_days: 90,
                private_session_days: 7,
                visibility: StatusPageVisibility::Public,
                lifecycle: StatusPageLifecycle::Active,
                archived_at: None,
                purge_after: None,
                created_at: TimestampMicros(1),
                updated_at: TimestampMicros(1),
            },
            overall_status: ComponentStatus::MajorOutage,
            components: vec![StatusPageComponent {
                id: component_id.clone(),
                org_id: Id("org-secret".into()),
                status_page_id: Id("page-secret".into()),
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
            component_status_events: vec![StatusPageComponentStatusEvent {
                id: Id("event-secret".into()),
                org_id: Id("org-secret".into()),
                status_page_id: Id("page-secret".into()),
                component_id: component_id.clone(),
                status: ComponentStatus::Operational,
                started_at: TimestampMicros(1),
                ended_at: None,
                created_at: TimestampMicros(1),
            }],
            active_incidents: vec![StatusPageIncident {
                id: Id("publication-public".into()),
                org_id: Id("org-secret".into()),
                status_page_id: Id("page-secret".into()),
                source_incident_id: Some(Id("incident-secret".into())),
                kind: StatusPageIncidentKind::Incident,
                title: "API unavailable".into(),
                impact: IncidentImpact::Critical,
                status: PublicIncidentStatus::Investigating,
                publication_state: StatusPagePublicationState::Published,
                draft_message: None,
                component_ids: vec![component_id],
                updates: vec![StatusPageIncidentUpdate {
                    id: Id("update-public".into()),
                    org_id: Id("org-secret".into()),
                    status_page_id: Id("page-secret".into()),
                    incident_id: Id("publication-public".into()),
                    status: PublicIncidentStatus::Investigating,
                    message: "We are investigating.".into(),
                    created_at: TimestampMicros(2),
                }],
                started_at: TimestampMicros(1),
                ended_at: None,
                published_at: Some(TimestampMicros(1)),
                created_at: TimestampMicros(1),
                updated_at: TimestampMicros(2),
            }],
            scheduled_maintenance: Vec::new(),
            history: Vec::new(),
            updated_at: TimestampMicros(2),
            generated_at: TimestampMicros(2),
        };

        let value = serde_json::to_value(PublicStatusPageSnapshot::from(snapshot))
            .expect("serialize public snapshot");
        let page = value.get("page").expect("page projection");
        assert_eq!(
            page.get("slug").and_then(|value| value.as_str()),
            Some("acme")
        );
        assert_eq!(page["languages"], serde_json::json!(["en-us", "zh-cn"]));
        assert_eq!(page["history_days"], 30);
        assert_eq!(page["visibility"], "public");
        for sensitive in ["id", "org_id", "custom_domain", "created_at", "updated_at"] {
            assert!(
                page.get(sensitive).is_none(),
                "public page leaked {sensitive}"
            );
        }
        let incident = &value["active_incidents"][0];
        for sensitive in [
            "org_id",
            "status_page_id",
            "source_incident_id",
            "created_at",
        ] {
            assert!(
                incident.get(sensitive).is_none(),
                "public incident leaked {sensitive}"
            );
        }
        assert_eq!(value["components"][0]["status"], "major_outage");
        assert_eq!(value["updated_at"], 2);
        let event = &value["component_status_events"][0];
        assert_eq!(event["component_id"], "component-public");
        for sensitive in ["id", "org_id", "status_page_id", "created_at"] {
            assert!(
                event.get(sensitive).is_none(),
                "status event leaked {sensitive}"
            );
        }
    }
}
