// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use sqlx::Row;

use crate::{
    domain::status_page::{
        ComponentKind, ComponentLifecycle, ComponentServiceLink, ComponentStatus,
        ComponentVisibility, IncidentImpact, PublicIncidentStatus, ServiceLinkMode, StatusPage,
        StatusPageComponent, StatusPageComponentStatusEvent, StatusPageIncident,
        StatusPageIncidentKind, StatusPageIncidentUpdate, StatusPageLifecycle,
        StatusPagePostmortem, StatusPagePublicationState, StatusPageVisibility,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub(super) const PAGE_COLS: &str = "id, org_id, name, slug, logo_url, brand_color,
    custom_domain, timezone, language, languages, history_days, visibility,
    delivery_retention_days, private_session_days, lifecycle, archived_at_micros,
    purge_after_micros,
    created_at_micros, updated_at_micros, slack_connector_id";

pub(super) const COMPONENT_COLS: &str = "id, org_id, status_page_id, name, description,
    status, visibility, lifecycle, position, archived_at_micros,
    created_at_micros, updated_at_micros,
    business_service_id, business_service_mode, business_service_notify, kind, group_id";

pub(super) const COMPONENT_STATUS_EVENT_COLS: &str = "id, org_id, status_page_id,
    component_id, status, started_at_micros, ended_at_micros, created_at_micros,
    notify_subscribers";

pub(super) const INCIDENT_COLS: &str = "id, org_id, status_page_id, source_incident_id,
    kind, title, impact, status, publication_state, draft_message, component_ids,
    started_at_micros, ended_at_micros, auto_start, reminder_minutes,
    published_at_micros, created_at_micros, updated_at_micros,
    postmortem_body, postmortem_state, postmortem_published_at_micros,
    postmortem_revised_at_micros, postmortem_updated_at_micros, postmortem_updated_by";

pub(super) const UPDATE_COLS: &str = "id, org_id, status_page_id, incident_id, status, message,
    created_at_micros, next_update_at_micros, notify_subscribers";

pub(super) fn row_to_page(row: sqlx::postgres::PgRow) -> Result<StatusPage> {
    let visibility: String = row.try_get("visibility").map_err(super::sqlx_err)?;
    let lifecycle: String = row.try_get("lifecycle").map_err(super::sqlx_err)?;
    let language: String = row.try_get("language").map_err(super::sqlx_err)?;
    let languages = row.try_get("languages").map_err(super::sqlx_err)?;

    Ok(StatusPage {
        id: Id(row.try_get("id").map_err(super::sqlx_err)?),
        org_id: Id(row.try_get("org_id").map_err(super::sqlx_err)?),
        name: row.try_get("name").map_err(super::sqlx_err)?,
        slug: row.try_get("slug").map_err(super::sqlx_err)?,
        logo_url: row.try_get("logo_url").map_err(super::sqlx_err)?,
        brand_color: row.try_get("brand_color").map_err(super::sqlx_err)?,
        custom_domain: row.try_get("custom_domain").map_err(super::sqlx_err)?,
        timezone: row.try_get("timezone").map_err(super::sqlx_err)?,
        language,
        languages,
        history_days: row.try_get("history_days").map_err(super::sqlx_err)?,
        delivery_retention_days: row
            .try_get("delivery_retention_days")
            .map_err(super::sqlx_err)?,
        private_session_days: row
            .try_get("private_session_days")
            .map_err(super::sqlx_err)?,
        visibility: StatusPageVisibility::parse(&visibility).ok_or_else(|| {
            Error::internal(format!("unknown status page visibility: {visibility}"))
        })?,
        lifecycle: StatusPageLifecycle::parse(&lifecycle).ok_or_else(|| {
            Error::internal(format!("unknown status page lifecycle: {lifecycle}"))
        })?,
        archived_at: row
            .try_get::<Option<i64>, _>("archived_at_micros")
            .map_err(super::sqlx_err)?
            .map(TimestampMicros),
        purge_after: row
            .try_get::<Option<i64>, _>("purge_after_micros")
            .map_err(super::sqlx_err)?
            .map(TimestampMicros),
        created_at: TimestampMicros(row.try_get("created_at_micros").map_err(super::sqlx_err)?),
        updated_at: TimestampMicros(row.try_get("updated_at_micros").map_err(super::sqlx_err)?),
        slack_connector_id: row
            .try_get::<Option<String>, _>("slack_connector_id")
            .map_err(super::sqlx_err)?
            .map(Id),
    })
}

pub(super) fn row_to_component(row: sqlx::postgres::PgRow) -> Result<StatusPageComponent> {
    let status: String = row.try_get("status").map_err(super::sqlx_err)?;
    let visibility: String = row.try_get("visibility").map_err(super::sqlx_err)?;
    let lifecycle: String = row.try_get("lifecycle").map_err(super::sqlx_err)?;
    let kind: String = row.try_get("kind").map_err(super::sqlx_err)?;

    Ok(StatusPageComponent {
        id: Id(row.try_get("id").map_err(super::sqlx_err)?),
        org_id: Id(row.try_get("org_id").map_err(super::sqlx_err)?),
        status_page_id: Id(row.try_get("status_page_id").map_err(super::sqlx_err)?),
        name: row.try_get("name").map_err(super::sqlx_err)?,
        description: row.try_get("description").map_err(super::sqlx_err)?,
        status: ComponentStatus::parse(&status)
            .ok_or_else(|| Error::internal(format!("unknown component status: {status}")))?,
        visibility: ComponentVisibility::parse(&visibility).ok_or_else(|| {
            Error::internal(format!("unknown component visibility: {visibility}"))
        })?,
        lifecycle: ComponentLifecycle::parse(&lifecycle)
            .ok_or_else(|| Error::internal(format!("unknown component lifecycle: {lifecycle}")))?,
        position: row.try_get("position").map_err(super::sqlx_err)?,
        archived_at: row
            .try_get::<Option<i64>, _>("archived_at_micros")
            .map_err(super::sqlx_err)?
            .map(TimestampMicros),
        created_at: TimestampMicros(row.try_get("created_at_micros").map_err(super::sqlx_err)?),
        updated_at: TimestampMicros(row.try_get("updated_at_micros").map_err(super::sqlx_err)?),
        business_service: row_to_service_link(&row)?,
        kind: ComponentKind::parse(&kind)
            .ok_or_else(|| Error::internal(format!("unknown component kind: {kind}")))?,
        group_id: row
            .try_get::<Option<String>, _>("group_id")
            .map_err(super::sqlx_err)?
            .map(Id),
    })
}

fn row_to_service_link(row: &sqlx::postgres::PgRow) -> Result<Option<ComponentServiceLink>> {
    let business_service_id: Option<String> = row
        .try_get("business_service_id")
        .map_err(super::sqlx_err)?;

    let Some(business_service_id) = business_service_id else {
        return Ok(None);
    };

    let mode: String = row
        .try_get("business_service_mode")
        .map_err(super::sqlx_err)?;

    Ok(Some(ComponentServiceLink {
        business_service_id: Id(business_service_id),
        mode: ServiceLinkMode::parse(&mode)
            .ok_or_else(|| Error::internal(format!("unknown business service link: {mode}")))?,
        notify_subscribers: row
            .try_get("business_service_notify")
            .map_err(super::sqlx_err)?,
    }))
}

pub(super) fn row_to_component_status_event(
    row: sqlx::postgres::PgRow,
) -> Result<StatusPageComponentStatusEvent> {
    let status: String = row.try_get("status").map_err(super::sqlx_err)?;
    let ended_at: Option<i64> = row.try_get("ended_at_micros").map_err(super::sqlx_err)?;

    Ok(StatusPageComponentStatusEvent {
        id: Id(row.try_get("id").map_err(super::sqlx_err)?),
        org_id: Id(row.try_get("org_id").map_err(super::sqlx_err)?),
        status_page_id: Id(row.try_get("status_page_id").map_err(super::sqlx_err)?),
        component_id: Id(row.try_get("component_id").map_err(super::sqlx_err)?),
        status: ComponentStatus::parse(&status)
            .ok_or_else(|| Error::internal(format!("unknown component status event: {status}")))?,
        started_at: TimestampMicros(row.try_get("started_at_micros").map_err(super::sqlx_err)?),
        ended_at: ended_at.map(TimestampMicros),
        created_at: TimestampMicros(row.try_get("created_at_micros").map_err(super::sqlx_err)?),
        notify_subscribers: row.try_get("notify_subscribers").map_err(super::sqlx_err)?,
    })
}

pub(super) fn row_to_incident(row: sqlx::postgres::PgRow) -> Result<StatusPageIncident> {
    let kind: String = row.try_get("kind").map_err(super::sqlx_err)?;
    let impact: String = row.try_get("impact").map_err(super::sqlx_err)?;
    let status: String = row.try_get("status").map_err(super::sqlx_err)?;
    let publication_state: String = row.try_get("publication_state").map_err(super::sqlx_err)?;
    let ended_at: Option<i64> = row.try_get("ended_at_micros").map_err(super::sqlx_err)?;
    let component_ids: Vec<String> = row.try_get("component_ids").map_err(super::sqlx_err)?;

    Ok(StatusPageIncident {
        id: Id(row.try_get("id").map_err(super::sqlx_err)?),
        org_id: Id(row.try_get("org_id").map_err(super::sqlx_err)?),
        status_page_id: Id(row.try_get("status_page_id").map_err(super::sqlx_err)?),
        source_incident_id: row
            .try_get::<Option<String>, _>("source_incident_id")
            .map_err(super::sqlx_err)?
            .map(Id),
        kind: StatusPageIncidentKind::parse(&kind)
            .ok_or_else(|| Error::internal(format!("unknown status incident kind: {kind}")))?,
        title: row.try_get("title").map_err(super::sqlx_err)?,
        impact: IncidentImpact::parse(&impact)
            .ok_or_else(|| Error::internal(format!("unknown status incident impact: {impact}")))?,
        status: PublicIncidentStatus::parse(&status)
            .ok_or_else(|| Error::internal(format!("unknown public incident status: {status}")))?,
        publication_state: StatusPagePublicationState::parse(&publication_state).ok_or_else(
            || Error::internal(format!("unknown publication state: {publication_state}")),
        )?,
        draft_message: row.try_get("draft_message").map_err(super::sqlx_err)?,
        component_ids: component_ids.into_iter().map(Id).collect(),
        updates: Vec::new(),
        started_at: TimestampMicros(row.try_get("started_at_micros").map_err(super::sqlx_err)?),
        ended_at: ended_at.map(TimestampMicros),
        auto_start: row.try_get("auto_start").map_err(super::sqlx_err)?,
        reminder_minutes: row.try_get("reminder_minutes").map_err(super::sqlx_err)?,
        published_at: row
            .try_get::<Option<i64>, _>("published_at_micros")
            .map_err(super::sqlx_err)?
            .map(TimestampMicros),
        created_at: TimestampMicros(row.try_get("created_at_micros").map_err(super::sqlx_err)?),
        updated_at: TimestampMicros(row.try_get("updated_at_micros").map_err(super::sqlx_err)?),
        postmortem: row_to_postmortem(&row)?,
    })
}

fn row_to_postmortem(row: &sqlx::postgres::PgRow) -> Result<Option<StatusPagePostmortem>> {
    let state: Option<String> = row.try_get("postmortem_state").map_err(super::sqlx_err)?;

    let Some(state) = state else {
        return Ok(None);
    };

    let micros = |column: &str| -> Result<Option<TimestampMicros>> {
        row.try_get::<Option<i64>, _>(column)
            .map(|value| value.map(TimestampMicros))
            .map_err(super::sqlx_err)
    };

    Ok(Some(StatusPagePostmortem {
        body: row
            .try_get::<Option<String>, _>("postmortem_body")
            .map_err(super::sqlx_err)?
            .unwrap_or_default(),
        publication_state: StatusPagePublicationState::parse(&state)
            .ok_or_else(|| Error::internal(format!("unknown postmortem state: {state}")))?,
        published_at: micros("postmortem_published_at_micros")?,
        revised_at: micros("postmortem_revised_at_micros")?,
        updated_by: row
            .try_get::<Option<String>, _>("postmortem_updated_by")
            .map_err(super::sqlx_err)?
            .map(Id),
        updated_at: micros("postmortem_updated_at_micros")?.unwrap_or(TimestampMicros(0)),
    }))
}

pub(super) fn row_to_update(row: sqlx::postgres::PgRow) -> Result<StatusPageIncidentUpdate> {
    let status: String = row.try_get("status").map_err(super::sqlx_err)?;

    Ok(StatusPageIncidentUpdate {
        id: Id(row.try_get("id").map_err(super::sqlx_err)?),
        org_id: Id(row.try_get("org_id").map_err(super::sqlx_err)?),
        status_page_id: Id(row.try_get("status_page_id").map_err(super::sqlx_err)?),
        incident_id: Id(row.try_get("incident_id").map_err(super::sqlx_err)?),
        status: PublicIncidentStatus::parse(&status)
            .ok_or_else(|| Error::internal(format!("unknown public incident status: {status}")))?,
        message: row.try_get("message").map_err(super::sqlx_err)?,
        created_at: TimestampMicros(row.try_get("created_at_micros").map_err(super::sqlx_err)?),
        next_update_at: row
            .try_get::<Option<i64>, _>("next_update_at_micros")
            .map_err(super::sqlx_err)?
            .map(TimestampMicros),
        notify_subscribers: row.try_get("notify_subscribers").map_err(super::sqlx_err)?,
    })
}
