// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 排班的覆盖时段：谁在什么时候值班，由后端按排班时区、夏令时和生效时段算好，前端只负责画。
//!
//! - 排班的列表和详情都带 `coverage`：从一天前到 14 天后的时段（[`schedule_view`]）；
//! - `GET /schedules/{id}/coverage?from=&to=`：任意一段，最长 90 天；
//! - `POST /schedules/coverage-preview`：编辑中、还没保存的排班的时段，给编辑抽屉预览用。

use axum::{
    Extension, Json, Router,
    extract::{Path, Query, State},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    api::AppState,
    app::iam::IamContext,
    domain::{
        alerting::schedule::{Rotation, Schedule, ScheduleOverride},
        iam::{permission, resource_permission},
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

const DAY_MICROS: i64 = 86_400_000_000;

/// 列表和详情里的覆盖时段从多久以前开始：一天。
const VIEW_BEFORE_MICROS: i64 = DAY_MICROS;

/// 列表和详情里的覆盖时段算到多久以后：14 天。
const VIEW_AFTER_MICROS: i64 = 14 * DAY_MICROS;

/// 一次最多算多长：90 天。
const MAX_RANGE_MICROS: i64 = 90 * DAY_MICROS;

/// 不给范围时预览多长：7 天。
const DEFAULT_PREVIEW_MICROS: i64 = 7 * DAY_MICROS;

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/schedules/{id}/coverage", get(coverage))
        .route("/schedules/coverage-preview", post(preview))
}

/// 排班本身加上从一天前到 14 天后的覆盖时段。
pub(super) fn schedule_view(schedule: &Schedule, now: TimestampMicros) -> Value {
    let from = TimestampMicros(now.0.saturating_sub(VIEW_BEFORE_MICROS));
    let to = TimestampMicros(now.0.saturating_add(VIEW_AFTER_MICROS));
    let mut body = serde_json::to_value(schedule).unwrap_or(Value::Null);

    if let Some(fields) = body.as_object_mut() {
        fields.insert("coverage".into(), coverage_body(schedule, from, to));
    }

    body
}

fn coverage_body(schedule: &Schedule, from: TimestampMicros, to: TimestampMicros) -> Value {
    json!({
        "from": from,
        "to": to,
        "segments": schedule.segments(from, to),
    })
}

#[derive(Deserialize)]
struct RangeQuery {
    from: Option<i64>,
    to: Option<i64>,
}

/// 给出的范围，或从现在起的 `default_micros`；结束要晚于开始，最长 90 天。
fn range(
    from: Option<i64>,
    to: Option<i64>,
    default_micros: i64,
) -> Result<(TimestampMicros, TimestampMicros)> {
    let from = from.unwrap_or_else(|| TimestampMicros::now().0);
    let to = to.unwrap_or_else(|| from.saturating_add(default_micros));

    if to <= from {
        return Err(Error::invalid("`to` must be later than `from`"));
    }

    if to - from > MAX_RANGE_MICROS {
        return Err(Error::invalid("coverage spans at most 90 days"));
    }

    Ok((TimestampMicros(from), TimestampMicros(to)))
}

#[resource_permission(
    action = "schedules.read",
    resource = Schedule,
    id = Id::from_string(id),
    bind = schedule
)]
async fn coverage(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Query(query): Query<RangeQuery>,
) -> Result<Json<Value>> {
    let (from, to) = range(query.from, query.to, VIEW_AFTER_MICROS)?;

    Ok(Json(coverage_body(&schedule, from, to)))
}

#[derive(Deserialize)]
struct PreviewReq {
    #[serde(default = "default_timezone")]
    timezone: String,
    #[serde(default = "default_enabled")]
    enabled: bool,
    #[serde(default)]
    rotations: Vec<Rotation>,
    #[serde(default)]
    overrides: Vec<ScheduleOverride>,
    from: Option<i64>,
    to: Option<i64>,
}

fn default_timezone() -> String {
    "UTC".to_string()
}

fn default_enabled() -> bool {
    true
}

#[permission("schedules.read")]
async fn preview(
    Extension(ctx): Extension<IamContext>,
    Json(req): Json<PreviewReq>,
) -> Result<Json<Value>> {
    let (from, to) = range(req.from, req.to, DEFAULT_PREVIEW_MICROS)?;
    let now = TimestampMicros::now();

    let draft = Schedule {
        id: Id::from_string("preview"),
        org_id: ctx.org_id.clone(),
        name: String::new(),
        description: String::new(),
        team_id: None,
        timezone: req.timezone,
        enabled: req.enabled,
        rotations: req.rotations,
        overrides: req.overrides,
        swap_needs_approval: false,
        created_by: None,
        updated_by: None,
        created_at: now,
        updated_at: now,
    };

    draft.validate()?;

    Ok(Json(coverage_body(&draft, from, to)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_range_defaults_from_now_and_stays_within_ninety_days() {
        assert!(range(Some(10), Some(10), DAY_MICROS).is_err());
        assert!(range(Some(0), Some(MAX_RANGE_MICROS + 1), DAY_MICROS).is_err());

        let (from, to) = range(Some(5), None, DAY_MICROS).unwrap();
        assert_eq!((from.0, to.0), (5, 5 + DAY_MICROS));
    }
}
