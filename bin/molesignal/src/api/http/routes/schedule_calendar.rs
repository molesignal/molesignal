// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 值班日历的订阅：
//!
//! - `GET/POST /schedules/calendar-feeds`、`DELETE /schedules/calendar-feeds/{id}`：自己的订阅，新建或
//!   重置时给出一次地址；
//! - `GET /api/v1/public/calendar/{token}.ics`：日历应用来读的地址，不带登录，靠地址里的 Token。每次读
//!   都按订阅人重新核对：账号和组织还能用，仍有 `schedules.read`；对不上一律 404。

use std::collections::HashMap;

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    http::{
        StatusCode,
        header::{CACHE_CONTROL, CONTENT_TYPE},
    },
    response::{IntoResponse, Response},
    routing::{delete, get},
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    agent::runs::RunPrincipal,
    api::{AppState, http::middleware::authorize_resource},
    app::{
        alerting::oncall::{CalendarTexts, render_ics},
        iam::IamContext,
    },
    domain::{
        alerting::{calendar::CalendarFeed, schedule::Schedule},
        i18n_language::FALLBACK_LANGUAGE_CODE,
        iam::{IamScope, permission},
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

const PUBLIC_PREFIX: &str = "/api/v1/public/calendar/";

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/schedules/calendar-feeds",
            get(list_feeds).post(create_feed),
        )
        .route("/schedules/calendar-feeds/{id}", delete(delete_feed))
}

pub fn public_routes() -> Router<AppState> {
    Router::new().route("/api/v1/public/calendar/{file}", get(calendar))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateFeedReq {
    /// 不填是个人订阅：我在各个排班里的班次。
    #[serde(default)]
    schedule_id: Option<String>,
}

#[permission("schedules.read")]
async fn list_feeds(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Value>> {
    let feeds = state
        .alerting
        .calendar
        .list(&ctx.org_id, &ctx.user_id)
        .await?;

    let names: HashMap<Id, String> = state
        .alerting
        .service
        .list_schedules(&ctx.org_id)
        .await?
        .into_iter()
        .map(|schedule| (schedule.id, schedule.name))
        .collect();

    let feeds: Vec<Value> = feeds.iter().map(|feed| feed_json(feed, &names)).collect();

    Ok(Json(json!({ "feeds": feeds })))
}

#[permission("schedules.calendar_feeds.manage")]
async fn create_feed(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(req): Json<CreateFeedReq>,
) -> Result<Json<Value>> {
    let schedule = match req
        .schedule_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
    {
        Some(id) => Some(
            authorize_resource::<Schedule>(&state, &ctx, Id::from_string(id), "schedules.read")
                .await?,
        ),
        None => None,
    };

    let (feed, token) = state
        .alerting
        .calendar
        .create(
            &ctx.org_id,
            &ctx.user_id,
            schedule.as_ref().map(|schedule| schedule.id.clone()),
        )
        .await?;

    let names: HashMap<Id, String> = schedule
        .into_iter()
        .map(|schedule| (schedule.id, schedule.name))
        .collect();

    let path = format!("{PUBLIC_PREFIX}{token}.ics");
    let base = state.platform.external_url.trim().trim_end_matches('/');

    Ok(Json(json!({
        "feed": feed_json(&feed, &names),
        "path": path,
        "url": (!base.is_empty()).then(|| format!("{base}{path}")),
    })))
}

#[permission("schedules.calendar_feeds.manage")]
async fn delete_feed(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    let deleted = state
        .alerting
        .calendar
        .delete(&ctx.org_id, &ctx.user_id, &Id::from_string(id))
        .await?;

    if !deleted {
        return Err(Error::not_found("calendar feed not found"));
    }

    Ok(Json(json!({ "deleted": true })))
}

fn feed_json(feed: &CalendarFeed, names: &HashMap<Id, String>) -> Value {
    json!({
        "id": feed.id,
        "scheduleId": feed.schedule_id,
        "scheduleName": feed.schedule_id.as_ref().and_then(|id| names.get(id)),
        "createdAt": feed.created_at,
        "lastUsedAt": feed.last_used_at,
    })
}

async fn calendar(State(state): State<AppState>, Path(file): Path<String>) -> Response {
    let token = file.strip_suffix(".ics").unwrap_or(&file);

    match render(&state, token).await {
        Ok(Some(body)) => (
            [
                (CONTENT_TYPE, "text/calendar; charset=utf-8"),
                (CACHE_CONTROL, "private, max-age=300"),
            ],
            body,
        )
            .into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(error) => {
            tracing::warn!(error = %error, "on-call calendar could not be written");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// 地址对应的日历；Token 不对、订阅人不能再看时为 `None`。
async fn render(state: &AppState, token: &str) -> Result<Option<String>> {
    let Some(feed) = state.alerting.calendar.find(token).await? else {
        return Ok(None);
    };

    let Some(ctx) = subscriber(state, &feed).await else {
        return Ok(None);
    };

    let mut names = HashMap::new();

    if let Some(schedule_id) = &feed.schedule_id {
        let Ok(schedule) =
            authorize_resource::<Schedule>(state, &ctx, schedule_id.clone(), "schedules.read")
                .await
        else {
            return Ok(None);
        };

        names = people_names(state, &schedule).await;
    }

    let texts = calendar_texts(state, &feed.org_id, &feed.user_id).await;
    let base = state.platform.external_url.trim().trim_end_matches('/');
    let now = TimestampMicros::now();

    let (name, events) = state
        .alerting
        .calendar
        .calendar(
            &feed,
            &texts,
            &names,
            |schedule_id| format!("{base}/alerts/schedules/{schedule_id}"),
            now,
        )
        .await?;

    Ok(Some(render_ics(&name, &events, now)))
}

/// 订阅人此刻在组织里的权限：账号与组织还能用、仍能看排班。
async fn subscriber(state: &AppState, feed: &CalendarFeed) -> Option<IamContext> {
    state
        .iam
        .service
        .ensure_user_access(&feed.user_id)
        .await
        .ok()?;
    state
        .iam
        .service
        .ensure_organization_access(&feed.org_id)
        .await
        .ok()?;

    let principal = RunPrincipal {
        user_id: feed.user_id.clone(),
        scope: IamScope::Organization,
        credential_token_id: None,
        credential_application_id: None,
        credential_service_account_id: None,
    };

    let mut context = principal.unresolved_context(&feed.org_id);
    state.iam.access.enrich_context(&mut context).await.ok()?;

    context.has_permission("schedules.read").then_some(context)
}

/// 排班里会出现的人的名字：轮换成员和替班的人。
async fn people_names(state: &AppState, schedule: &Schedule) -> HashMap<Id, String> {
    let mut ids: Vec<Id> = Vec::new();

    for id in schedule
        .rotations
        .iter()
        .flat_map(|rotation| rotation.members.iter())
        .chain(schedule.overrides.iter().map(|ov| &ov.user_id))
    {
        if !ids.contains(id) {
            ids.push(id.clone());
        }
    }

    let users = state
        .iam
        .service
        .users
        .get_many(&ids)
        .await
        .unwrap_or_default();

    users
        .into_iter()
        .map(|user| {
            let name = if user.display_name.trim().is_empty() {
                user.email.clone()
            } else {
                user.display_name.clone()
            };

            (user.id, name)
        })
        .collect()
}

/// 日历里的几句话，用订阅人的界面语言；没设时用工作区的默认。
async fn calendar_texts(state: &AppState, org_id: &Id, user_id: &Id) -> CalendarTexts {
    let personal = state
        .iam
        .user_preferences
        .get_optional(user_id)
        .await
        .ok()
        .flatten()
        .map(|preference| preference.language);

    let language = match personal {
        Some(language) => language,
        None => state
            .iam
            .workspace_preference_defaults
            .get(org_id)
            .await
            .map(|defaults| defaults.language)
            .unwrap_or_else(|_| FALLBACK_LANGUAGE_CODE.to_string()),
    };

    let pack = state
        .platform
        .i18n_language
        .load_pack("alerting.calendar.", &language)
        .await
        .unwrap_or_default();

    let text = |key: &str, fallback: &str| {
        pack.get(&format!("alerting.calendar.{key}"))
            .unwrap_or(fallback)
            .to_string()
    };

    CalendarTexts {
        person_name: text("person_name", "My on-call shifts"),
        person_summary: text("person_summary", "On call: {schedule}"),
        schedule_summary: text("schedule_summary", "{person} on call"),
    }
}
