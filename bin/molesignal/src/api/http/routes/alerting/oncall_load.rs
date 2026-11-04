// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `GET /alerts/insights/oncall-load`：每个人的值班负担（值班时长、告警通知、夜里与周末、被吵醒的
//! 晚上，以及先接手的事件）。

use axum::{
    Extension, Json, Router,
    extract::{Query, State},
    routing::get,
};
use serde::{Deserialize, Serialize};

use crate::{
    api::AppState,
    app::{
        alerting::insights::{NIGHT_END_HOUR, NIGHT_START_HOUR, PersonLoad},
        iam::IamContext,
    },
    domain::iam::permission,
    shared::{Result, ids::Id, time::TimestampMicros},
};

pub(super) fn routes() -> Router<AppState> {
    Router::new().route("/alerts/insights/oncall-load", get(get_oncall_load))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OncallLoadQuery {
    /// 往回看多久（秒），默认 30 天，限在 1 天到 90 天。
    window_secs: Option<u64>,
    /// 只算这个排班。
    schedule_id: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct OncallLoadResponse {
    window_secs: u64,
    night_start_hour: u32,
    night_end_hour: u32,
    people: Vec<PersonLoad>,
}

#[permission("alerts.incidents.read")]
async fn get_oncall_load(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(q): Query<OncallLoadQuery>,
) -> Result<Json<OncallLoadResponse>> {
    let window_secs = q
        .window_secs
        .unwrap_or(30 * 24 * 3600)
        .clamp(24 * 3600, 90 * 24 * 3600);

    let now = TimestampMicros::now();
    let since = TimestampMicros(now.0 - (window_secs as i64) * 1_000_000);
    let schedule = q
        .schedule_id
        .filter(|id| !id.is_empty())
        .map(Id::from_string);

    let people = state
        .alerting
        .insights
        .oncall_load(&ctx.org_id, since, now, schedule.as_ref())
        .await?;

    Ok(Json(OncallLoadResponse {
        window_secs,
        night_start_hour: NIGHT_START_HOUR,
        night_end_hour: NIGHT_END_HOUR,
        people,
    }))
}
