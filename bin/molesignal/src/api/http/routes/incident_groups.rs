// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Incident groups HTTP routes（spec incidents-grouping）。

use axum::{
    Extension, Json, Router,
    extract::{Path, Query, State},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};

use crate::{
    api::AppState,
    app::iam::IamContext,
    domain::{
        alerting::{
            incident::Incident,
            incident_group::{IncidentGroup, IncidentGroupState},
        },
        iam::permission,
    },
    shared::{Result, ids::Id, time::TimestampMicros},
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/alerts/incident_groups", get(list))
        .route("/alerts/incident_groups/{id}", get(get_one))
        .route("/alerts/incident_groups/{id}/ack", post(ack))
        .route("/alerts/incident_groups/{id}/resolve", post(resolve))
}

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    pub state: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: i64,
}
fn default_limit() -> i64 {
    200
}

#[derive(Debug, Serialize)]
pub struct GroupResp {
    pub id: String,
    pub alert_rule_id: String,
    pub fingerprint: String,
    pub state: String,
    pub incident_count: i32,
    pub first_at_micros: i64,
    pub last_at_micros: i64,
    pub acked_by: Option<String>,
    pub acked_at_micros: Option<i64>,
    pub resolved_at_micros: Option<i64>,
}

fn to_resp(g: IncidentGroup) -> GroupResp {
    GroupResp {
        id: g.id.0,
        alert_rule_id: g.alert_rule_id.0,
        fingerprint: g.fingerprint,
        state: g.state.as_str().to_string(),
        incident_count: g.incident_count,
        first_at_micros: g.first_at.0,
        last_at_micros: g.last_at.0,
        acked_by: g.acked_by.map(|i| i.0),
        acked_at_micros: g.acked_at.map(|t| t.0),
        resolved_at_micros: g.resolved_at.map(|t| t.0),
    }
}

#[permission("alerts.read")]
async fn list(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(p): Query<ListQuery>,
) -> Result<Json<Vec<GroupResp>>> {
    let filter = p.state.as_deref().map(IncidentGroupState::parse);
    let groups = state
        .alerting
        .incident_groups
        .list(&ctx.org_id, filter, p.limit.clamp(1, 1000))
        .await?;
    Ok(Json(groups.into_iter().map(to_resp).collect()))
}

#[permission("alerts.read")]
async fn get_one(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<GroupResp>> {
    let g = state
        .alerting
        .incident_groups
        .get(&ctx.org_id, &Id(id))
        .await?;
    Ok(Json(to_resp(g)))
}

#[permission("alerts.acknowledge")]
async fn ack(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    let group_id = Id(id);
    let now = TimestampMicros::now();
    state
        .alerting
        .incident_groups
        .ack(&ctx.org_id, &group_id, &ctx.user_id, now)
        .await?;
    // 把 group 上的 ack 级联到成员 incident（best-effort）。
    let group = state
        .alerting
        .incident_groups
        .get(&ctx.org_id, &group_id)
        .await?;
    let members = member_incidents(&state, &ctx.org_id, &group).await?;
    let mut acked = 0usize;
    for inc in &members {
        if state
            .alerting
            .service
            .acknowledge(&inc.id, ctx.user_id.clone(), now)
            .await
            .is_ok()
        {
            acked += 1;
        }
    }
    Ok(Json(serde_json::json!({
        "acked": true,
        "incident_acked": acked > 0,
        "incidents_acked": acked,
    })))
}

/// group 级 ack/resolve 要级联到的成员 incident。
///
/// - group.alert_rule_id 是一条语义分组 id（`semantic_groups.get` 命中且同 org）：该
///   group 是跨规则聚合，成员 = 所有 active incident 中被该语义分组命中、且 group_by
///   派生 key == group.fingerprint 的（1:many）。
/// - 否则是逐规则去重簇（scope = rule id，fingerprint = incident 自身 fingerprint）：
///   按 fingerprint 取唯一 incident（0/1）。
///
/// 注：语义分支会扫描该 org 的 active incident（manual ack/resolve 罕发，可接受）。
async fn member_incidents(
    state: &AppState,
    org: &Id,
    group: &IncidentGroup,
) -> Result<Vec<Incident>> {
    if let Ok(sg) = state
        .alerting
        .semantic_groups
        .get(&group.alert_rule_id)
        .await
        && sg.org_id == *org
    {
        let actives = state.alerting.service.incidents.list_active(org).await?;
        return Ok(actives
            .into_iter()
            .filter(|i| sg.matches(&i.labels) && sg.group_key(&i.labels) == group.fingerprint)
            .collect());
    }
    Ok(state
        .alerting
        .service
        .incidents
        .find_by_fingerprint(org, &group.fingerprint)
        .await?
        .into_iter()
        .collect())
}

#[permission("alerts.acknowledge")]
async fn resolve(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    let group_id = Id(id);
    let now = TimestampMicros::now();
    state
        .alerting
        .incident_groups
        .resolve(&ctx.org_id, &group_id, now)
        .await?;
    // group resolve → 成员 incident resolve（best-effort）。
    // 逐规则去重簇是 1:1（按 incident fingerprint）；语义分组是 1:many（跨规则聚合，
    // 见 member_incidents）。
    let group = state
        .alerting
        .incident_groups
        .get(&ctx.org_id, &group_id)
        .await?;
    let members = member_incidents(&state, &ctx.org_id, &group).await?;
    let mut resolved = 0usize;
    for inc in &members {
        if state
            .alerting
            .service
            .resolve(&inc.id, ctx.user_id.clone(), now)
            .await
            .is_ok()
        {
            resolved += 1;
        }
    }
    Ok(Json(serde_json::json!({
        "resolved": true,
        "incident_resolved": resolved > 0,
        "incidents_resolved": resolved,
    })))
}
