// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 节假日日历（读要 `alerts.calendars.read`，改要 `alerts.calendars.manage`）：
//!
//! - `GET /api/v1/alerts/holiday-calendars`、`POST`（`{name, description, days}`）；
//! - `GET|PUT|DELETE /api/v1/alerts/holiday-calendars/{id}`：被引用着的不能删（409）；
//! - `GET /api/v1/alerts/holiday-calendars/{id}/references`：哪些地方引用了它；
//! - `POST /api/v1/alerts/holiday-calendars/parse`（`{content}`）：读 ICS，返回每一天，只给预览，
//!   不存。

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    api::{
        AppState,
        http::{
            federation::{delete_payload, emit_cud},
            routes::activity_audit,
        },
    },
    app::{alerting::holidays::CalendarInput, iam::IamContext},
    domain::{
        alerting::holiday::{CalendarReference, HolidayCalendar, HolidayDay, parse_ics},
        federation::{CudAction, ResourceKind},
        iam::permission,
    },
    shared::{Error, Result, ids::Id},
};

/// 一次最多读多大的 ICS。
const MAX_ICS_BYTES: usize = 2 * 1024 * 1024;

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/alerts/holiday-calendars",
            get(list_calendars).post(create_calendar),
        )
        .route("/alerts/holiday-calendars/parse", post(parse_calendar))
        .route(
            "/alerts/holiday-calendars/{id}",
            get(get_calendar)
                .put(update_calendar)
                .delete(delete_calendar),
        )
        .route(
            "/alerts/holiday-calendars/{id}/references",
            get(calendar_references),
        )
}

#[derive(Debug, Deserialize)]
struct CalendarRequest {
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    days: Vec<HolidayDay>,
}

impl From<CalendarRequest> for CalendarInput {
    fn from(request: CalendarRequest) -> Self {
        Self {
            name: request.name,
            description: request.description,
            days: request.days,
        }
    }
}

#[derive(Debug, Deserialize)]
struct ParseRequest {
    content: String,
}

#[permission("alerts.calendars.read")]
async fn list_calendars(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Vec<HolidayCalendar>>> {
    Ok(Json(state.alerting.holidays.list(&ctx.org_id).await?))
}

#[permission("alerts.calendars.read")]
async fn get_calendar(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<HolidayCalendar>> {
    Ok(Json(
        state.alerting.holidays.get(&ctx.org_id, &Id(id)).await?,
    ))
}

#[permission("alerts.calendars.manage")]
async fn create_calendar(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(request): Json<CalendarRequest>,
) -> Result<Json<HolidayCalendar>> {
    let created = state
        .alerting
        .holidays
        .create(&ctx.org_id, Some(ctx.user_id.clone()), request.into())
        .await?;

    emit_cud(
        &state,
        &ctx.org_id,
        ResourceKind::HolidayCalendar,
        CudAction::Created,
        &created.id.0,
        &created,
    )
    .await;
    audit(&state, &ctx, "holiday_calendar.created", &created).await;

    Ok(Json(created))
}

#[permission("alerts.calendars.manage")]
async fn update_calendar(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(request): Json<CalendarRequest>,
) -> Result<Json<HolidayCalendar>> {
    let updated = state
        .alerting
        .holidays
        .update(&ctx.org_id, &Id(id), request.into())
        .await?;

    emit_cud(
        &state,
        &ctx.org_id,
        ResourceKind::HolidayCalendar,
        CudAction::Updated,
        &updated.id.0,
        &updated,
    )
    .await;
    audit(&state, &ctx, "holiday_calendar.updated", &updated).await;

    Ok(Json(updated))
}

#[permission("alerts.calendars.manage")]
async fn delete_calendar(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    let deleted = state.alerting.holidays.delete(&ctx.org_id, &Id(id)).await?;

    emit_cud(
        &state,
        &ctx.org_id,
        ResourceKind::HolidayCalendar,
        CudAction::Deleted,
        &deleted.id.0,
        &delete_payload(&deleted.id.0),
    )
    .await;
    audit(&state, &ctx, "holiday_calendar.deleted", &deleted).await;

    Ok(Json(json!({ "deleted": true })))
}

#[permission("alerts.calendars.read")]
async fn calendar_references(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Vec<CalendarReference>>> {
    Ok(Json(
        state
            .alerting
            .holidays
            .references(&ctx.org_id, &Id(id))
            .await?,
    ))
}

#[permission("alerts.calendars.manage")]
async fn parse_calendar(
    State(_state): State<AppState>,
    Extension(_ctx): Extension<IamContext>,
    Json(request): Json<ParseRequest>,
) -> Result<Json<Value>> {
    if request.content.len() > MAX_ICS_BYTES {
        return Err(Error::invalid("the file is larger than 2 MiB"));
    }

    let days = parse_ics(&request.content)?;

    Ok(Json(json!({ "days": days })))
}

async fn audit(state: &AppState, ctx: &IamContext, action: &str, calendar: &HolidayCalendar) {
    activity_audit::record(
        state,
        ctx,
        action,
        "holiday_calendar",
        &calendar.id.0,
        json!({ "name": calendar.name, "days": calendar.days.len() }),
    )
    .await;
}
