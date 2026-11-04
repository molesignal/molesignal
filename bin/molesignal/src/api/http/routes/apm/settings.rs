// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! An organization's APM settings: read by whoever reads APM, set by whoever
//! manages it. Saved settings apply on this node at once and on the others
//! within half a minute; naming rules and the environment mapping to the
//! spans read from then on.

use axum::{Extension, Json, Router, extract::State, routing::get};

use crate::{
    api::{AppState, http::routes::activity_audit},
    app::iam::IamContext,
    domain::{apm::ApmOrgSettings, iam::permission},
    shared::Result,
};

pub(super) fn routes() -> Router<AppState> {
    Router::new().route("/apm/settings", get(get_settings).put(put_settings))
}

#[permission(any("apm.settings.read", "sys.telemetry.read"))]
async fn get_settings(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<ApmOrgSettings>> {
    Ok(Json(
        state
            .telemetry
            .apm_catalog
            .get_settings(&ctx.org_id)
            .await?
            .unwrap_or_default(),
    ))
}

#[permission("apm.settings.manage")]
async fn put_settings(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(mut settings): Json<ApmOrgSettings>,
) -> Result<Json<ApmOrgSettings>> {
    settings.normalize();
    settings.validate()?;

    state
        .telemetry
        .apm_catalog
        .save_settings(&ctx.org_id, &settings, ctx.principal_id())
        .await?;

    if let Some(book) = state
        .telemetry
        .apm_runtime
        .as_ref()
        .and_then(|runtime| runtime.org_settings())
    {
        book.replace_org(&ctx.org_id, &settings);
    }

    state.telemetry.apm_query.forget_settings(&ctx.org_id);

    activity_audit::record(
        &state,
        &ctx,
        "apm.settings.update",
        "apm_settings",
        ctx.org_id.as_str(),
        serde_json::json!({
            "health": settings.health,
            "apdexThreshold": settings.apdex_threshold,
            "longTaskTime": settings.long_task_time,
            "namingRules": settings.naming_rules.len(),
            "environmentMappings": settings.environment_map.len(),
            "aliases": settings.aliases.len(),
            "metadataRules": settings.metadata_rules,
        }),
    )
    .await;

    Ok(Json(settings))
}
