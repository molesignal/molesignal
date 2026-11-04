// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `GET /notify/reachability?user_ids=a,b`：这些人收不收得到告警（自己的通知方式、只能靠群、收不到），
//! 排班、升级策略和个人设置页用来提醒。只回状态，不回通知方式本身。

use axum::{
    Extension, Json, Router,
    extract::{Query, State},
    routing::get,
};
use serde::{Deserialize, Serialize};

use crate::{
    api::AppState,
    app::{iam::IamContext, notify::Reachability},
    domain::iam::permission,
    shared::{Error, Result, ids::Id},
};

/// 一次最多问多少人。
const MAX_USERS: usize = 200;

pub(super) fn routes() -> Router<AppState> {
    Router::new().route("/notify/reachability", get(reachability))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReachabilityQuery {
    /// 逗号分隔的用户 id。
    #[serde(default)]
    user_ids: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct UserReachability {
    user_id: Id,
    status: Reachability,
}

#[derive(Serialize)]
struct ReachabilityResponse {
    users: Vec<UserReachability>,
}

#[permission("notify.policies.read")]
async fn reachability(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(query): Query<ReachabilityQuery>,
) -> Result<Json<ReachabilityResponse>> {
    let user_ids: Vec<Id> = query
        .user_ids
        .split(',')
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(Id::from_string)
        .collect();

    if user_ids.len() > MAX_USERS {
        return Err(Error::invalid(format!(
            "ask about at most {MAX_USERS} people at once"
        )));
    }

    let users = state
        .alerting
        .notify_engine
        .reachability(&ctx.org_id, &user_ids)
        .await?
        .into_iter()
        .map(|(user_id, status)| UserReachability { user_id, status })
        .collect();

    Ok(Json(ReachabilityResponse { users }))
}
