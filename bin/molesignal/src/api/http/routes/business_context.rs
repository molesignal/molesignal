// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The organization's business context: the keys it reads its telemetry by (a tenant, a plan, an
//! experiment group) and the field each signal carries them in. Read by whoever reads any signal,
//! since every page that splits by a key asks for them; set by whoever manages APM settings.
//!
//! - `GET /business-context`
//! - `PUT /business-context`

use axum::{Extension, Json, Router, extract::State, routing::get};
use domain::{
    business_context::BusinessContext,
    iam::{IamContext, permission},
};
use kernel::{Result, time::TimestampMicros};

use crate::api::{AppState, http::routes::activity_audit};

pub fn routes() -> Router<AppState> {
    Router::new().route("/business-context", get(get_context).put(put_context))
}

#[permission(any(
    "apm.services.read",
    "traces.read",
    "logs.read",
    "rum.events.read",
    "apm.settings.read",
    "sys.telemetry.read"
))]
async fn get_context(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<BusinessContext>> {
    Ok(Json(
        state
            .telemetry
            .business_context
            .business_context(&ctx.org_id)
            .await?,
    ))
}

#[permission("apm.settings.manage")]
async fn put_context(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(mut context): Json<BusinessContext>,
) -> Result<Json<BusinessContext>> {
    context.normalize();
    context.validate()?;

    state
        .telemetry
        .business_context
        .save_business_context(
            &ctx.org_id,
            &context,
            ctx.principal_id(),
            TimestampMicros::now(),
        )
        .await?;

    activity_audit::record(
        &state,
        &ctx,
        "business_context.update",
        "business_context",
        ctx.org_id.as_str(),
        serde_json::json!({
            "keys": context.keys.iter().map(|key| key.key.as_str()).collect::<Vec<_>>(),
        }),
    )
    .await;

    Ok(Json(context))
}
