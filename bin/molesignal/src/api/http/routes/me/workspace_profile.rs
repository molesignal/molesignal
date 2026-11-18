// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 组织的门面：`GET`、`PUT /workspace/profile` 读、改当前组织的 logo 与描述，改了记审计。组织列表
//! （`GET /orgs`）把每个组织的门面一并带上，顶栏的组织切换不用另外请求。

use axum::{Extension, Json, Router, extract::State, routing::get};
use domain::iam::{IamContext, permission};
use kernel::{Result, time::TimestampMicros};
use postgres::repositories::org_profiles::OrgProfile;

use crate::api::{AppState, http::routes::activity_audit};

pub(super) fn routes() -> Router<AppState> {
    Router::new().route("/workspace/profile", get(read).put(write))
}

#[permission("org.preferences.read")]
async fn read(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<OrgProfile>> {
    state.iam.org_profiles.get(&ctx.org_id).await.map(Json)
}

#[permission("org.preferences.manage")]
async fn write(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(profile): Json<OrgProfile>,
) -> Result<Json<OrgProfile>> {
    let profile = profile.normalize()?;

    state
        .iam
        .org_profiles
        .put(&ctx.org_id, &profile, &ctx.user_id, TimestampMicros::now())
        .await?;

    // The logo itself is an image of up to 64 KiB: the record says whether there is one.
    activity_audit::record(
        &state,
        &ctx,
        "org.profile.update",
        "organization",
        ctx.org_id.as_str(),
        serde_json::json!({
            "hasLogo": profile.logo.is_some(),
            "description": profile.description,
        }),
    )
    .await;

    Ok(Json(profile))
}
