// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use sqlx::Row;

use crate::{
    domain::status_page::{
        ComponentLifecycle, ComponentStatus, ComponentVisibility, IncidentImpact,
        PublicIncidentStatus, StatusPage, StatusPageComponent, StatusPageComponentStatusEvent,
        StatusPageIncident, StatusPageIncidentKind, StatusPageIncidentUpdate, StatusPageLifecycle,
        StatusPagePublicationState, StatusPageVisibility,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub(super) const PAGE_COLS: &str = "id, org_id, name, slug, logo_url, brand_color,
    custom_domain, timezone, language, languages, history_days, visibility,
    delivery_retention_days, private_session_days, lifecycle, archived_at_micros,
    purge_after_micros,
    created_at_micros, updated_at_micros";
pub(super) const COMPONENT_COLS: &str = "id, org_id, status_page_id, name, description,
    status, visibility, lifecycle, position, archived_at_micros,
    created_at_micros, updated_at_micros";
pub(super) const COMPONENT_STATUS_EVENT_COLS: &str = "id, org_id, status_page_id,
    component_id, status, started_at_micros, ended_at_micros, created_at_micros";
pub(super) const INCIDENT_COLS: &str = "id, org_id, status_page_id, source_incident_id,
    kind, title, impact, status, publication_state, draft_message,
    started_at_micros, ended_at_micros,
    published_at_micros, created_at_micros, updated_at_micros";
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
    })
}

pub(super) fn row_to_component(row: sqlx::postgres::PgRow) -> Result<StatusPageComponent> {
    let status: String = row.try_get("status").map_err(super::sqlx_err)?;
    let visibility: String = row.try_get("visibility").map_err(super::sqlx_err)?;
    let lifecycle: String = row.try_get("lifecycle").map_err(super::sqlx_err)?;
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
    })
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
    })
}

pub(super) fn row_to_incident(row: sqlx::postgres::PgRow) -> Result<StatusPageIncident> {
    let kind: String = row.try_get("kind").map_err(super::sqlx_err)?;
    let impact: String = row.try_get("impact").map_err(super::sqlx_err)?;
    let status: String = row.try_get("status").map_err(super::sqlx_err)?;
    let publication_state: String = row.try_get("publication_state").map_err(super::sqlx_err)?;
    let ended_at: Option<i64> = row.try_get("ended_at_micros").map_err(super::sqlx_err)?;
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
        component_ids: Vec::new(),
        updates: Vec::new(),
        started_at: TimestampMicros(row.try_get("started_at_micros").map_err(super::sqlx_err)?),
        ended_at: ended_at.map(TimestampMicros),
        published_at: row
            .try_get::<Option<i64>, _>("published_at_micros")
            .map_err(super::sqlx_err)?
            .map(TimestampMicros),
        created_at: TimestampMicros(row.try_get("created_at_micros").map_err(super::sqlx_err)?),
        updated_at: TimestampMicros(row.try_get("updated_at_micros").map_err(super::sqlx_err)?),
    })
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
    })
}
