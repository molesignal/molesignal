// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The applications that send RUM data.
//!
//! - `GET /rum/applications` lists them, `GET /rum/applications/{id}` reads one;
//! - `POST /rum/applications` creates one, `PATCH` changes its name, platform or settings,
//!   `DELETE` removes it and revokes its client token.
//!
//! An application is created before any SDK sends to it: an event names the application its
//! client token is bound to, and one for an application that does not exist is refused. Its client
//! token is issued by `GET /auth/tokens/rum?applicationId=…`. The data an application sent stays
//! in the streams after it is deleted, until their retention ends it.

use axum::{
    Extension, Json,
    extract::{Path, State},
};
use serde::{Deserialize, Serialize};

use crate::{
    api::{AppState, http::middleware::Permission},
    app::{dashboard::builtin::RumDashboard, iam::IamContext},
    domain::{
        iam::{api_token::ApiTokenKind, permission},
        rum::{
            RumApplication, RumApplicationPatch, RumApplicationSettings, RumPlatform,
            RumSdkSettings, resolve_id, validate_name,
        },
    },
    shared::{Error, Result, time::TimestampMicros},
};

/// What the settings of its application decide for an SDK that asks at start. A client token reads
/// this with what it writes with, and it says nothing an SDK would not be given in a page anyway.
#[permission("rum.write")]
pub(super) async fn sdk_config(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<RumSdkSettings>> {
    let application = ctx
        .credential_application_id
        .as_deref()
        .ok_or_else(|| Error::invalid("the configuration of an SDK is read with a client token"))?;

    let found = state
        .telemetry
        .rum_applications
        .get(&ctx.org_id, application)
        .await?;

    Ok(Json(found.settings.sdk))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ApplicationView {
    id: String,
    name: String,
    platform: RumPlatform,
    settings: RumApplicationSettings,
    created_by: Option<String>,
    created_at: i64,
    updated_at: i64,
}

impl From<RumApplication> for ApplicationView {
    fn from(application: RumApplication) -> Self {
        Self {
            id: application.id,
            name: application.name,
            platform: application.platform,
            settings: application.settings,
            created_by: application.created_by.map(|id| id.0),
            created_at: application.created_at.0,
            updated_at: application.updated_at.0,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CreateApplication {
    name: String,
    /// The identifier SDKs are configured with. Made from the name when left out; fixed after.
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    platform: Option<RumPlatform>,
    #[serde(default)]
    settings: Option<RumApplicationSettings>,
}

#[permission(any("rum.applications.read", "sys.telemetry.read"))]
pub(super) async fn list(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Vec<ApplicationView>>> {
    let applications = state
        .telemetry
        .rum_applications
        .list(&ctx.org_id)
        .await?
        .into_iter()
        .map(ApplicationView::from)
        .collect();

    Ok(Json(applications))
}

#[permission(any("rum.applications.read", "sys.telemetry.read"))]
pub(super) async fn get(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<ApplicationView>> {
    let application = state
        .telemetry
        .rum_applications
        .get(&ctx.org_id, &id)
        .await?;

    Ok(Json(application.into()))
}

pub(super) async fn create(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(body): Json<CreateApplication>,
) -> Result<Json<ApplicationView>> {
    require_manage(&ctx)?;
    validate_name(&body.name)?;

    // Stored checked, each retention filter named by an id.
    let settings = body.settings.unwrap_or_default().normalized()?;

    let now = TimestampMicros::now();

    let application = state
        .telemetry
        .rum_applications
        .create(RumApplication {
            org_id: ctx.org_id.clone(),
            id: resolve_id(body.id.as_deref(), &body.name)?,
            name: body.name.trim().to_string(),
            platform: body.platform.unwrap_or(RumPlatform::Browser),
            settings,
            created_by: Some(ctx.user_id.clone()),
            created_at: now,
            updated_at: now,
        })
        .await?;

    // The streams are created now so that the pages read them empty instead of missing. They are
    // created on the first event too, so a failure here does not undo the application.
    if let Err(error) = state
        .telemetry
        .rum_intake
        .provision_streams(&ctx.org_id)
        .await
    {
        tracing::warn!(%error, org_id = %ctx.org_id.0, "RUM streams could not be prepared");
    }

    state
        .telemetry
        .rum_intake
        .forget(&ctx.org_id, &application.id)
        .await;

    install_dashboard(&state, &ctx, application.platform).await;

    Ok(Json(application.into()))
}

/// The dashboard of a platform, for an organization that now has an application of it. The
/// application is saved either way, so a dashboard that cannot be installed is logged.
async fn install_dashboard(state: &AppState, ctx: &IamContext, platform: RumPlatform) {
    let Some(dashboard) = RumDashboard::of(platform) else {
        return;
    };

    if let Err(error) = state.dashboard.install_rum(&ctx.org_id, dashboard).await {
        tracing::warn!(%error, org_id = %ctx.org_id.0, "the RUM dashboard could not be installed");
    }
}

pub(super) async fn update(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(patch): Json<RumApplicationPatch>,
) -> Result<Json<ApplicationView>> {
    require_manage(&ctx)?;
    patch.validate()?;

    let application = state
        .telemetry
        .rum_applications
        .update(&ctx.org_id, &id, patch)
        .await?;

    state.telemetry.rum_intake.forget(&ctx.org_id, &id).await;

    install_dashboard(&state, &ctx, application.platform).await;

    Ok(Json(application.into()))
}

pub(super) async fn delete(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    require_manage(&ctx)?;

    state
        .telemetry
        .rum_applications
        .delete(&ctx.org_id, &id)
        .await?;

    state.telemetry.rum_intake.forget(&ctx.org_id, &id).await;

    // A deleted application takes its client token with it: the token could only write to it.
    let tokens = state.iam.api_tokens.list_by_org(&ctx.org_id).await?;

    for token in tokens {
        if token.token_kind == ApiTokenKind::RumClient
            && !token.revoked
            && token.application_id.as_deref() == Some(id.as_str())
        {
            state
                .iam
                .api_tokens
                .mark_revoked(&ctx.org_id, &token.id)
                .await?;
        }
    }

    Ok(Json(serde_json::json!({ "deleted": true })))
}

/// Managing an application is managing the client token it comes with.
fn require_manage(ctx: &IamContext) -> Result<()> {
    if ctx.is_system_scope() {
        return Err(Error::not_found("RUM applications"));
    }

    Permission::require_key(ctx, "rum.applications.manage")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::ids::Id;

    #[test]
    fn an_application_is_shown_with_its_times_in_microseconds() {
        let view = ApplicationView::from(RumApplication {
            org_id: Id::from_string("org"),
            id: "shop-web".into(),
            name: "Shop".into(),
            platform: RumPlatform::Browser,
            settings: RumApplicationSettings::default(),
            created_by: Some(Id::from_string("user-1")),
            created_at: TimestampMicros(10),
            updated_at: TimestampMicros(20),
        });

        let value = serde_json::to_value(view).unwrap();

        assert_eq!(value["id"], "shop-web");
        assert_eq!(value["platform"], "browser");
        assert_eq!(value["createdBy"], "user-1");
        assert_eq!(value["createdAt"], 10);
        assert_eq!(value["updatedAt"], 20);
        assert_eq!(value["settings"]["collectIp"], true);
        assert_eq!(value["settings"]["sessionSampleRate"], 100);
        assert_eq!(value["settings"]["keepErrors"], true);
        assert_eq!(value["settings"]["retentionFilters"], serde_json::json!([]));
        assert_eq!(
            value["settings"]["scrub"],
            serde_json::json!([
                "secrets",
                "emails",
                "phones",
                "cards",
                "national_ids",
                "url_params"
            ])
        );
        assert!(value.get("orgId").is_none());
    }

    #[test]
    fn a_new_application_with_a_share_of_sessions_beyond_all_of_them_is_refused() {
        let body: CreateApplication = serde_json::from_value(serde_json::json!({
            "name": "Shop",
            "settings": { "sessionSampleRate": 150 }
        }))
        .unwrap();

        assert!(body.settings.unwrap().validate().is_err());
    }

    #[test]
    fn a_new_application_may_leave_everything_but_the_name() {
        let body: CreateApplication =
            serde_json::from_value(serde_json::json!({ "name": "Shop" })).unwrap();

        assert_eq!(body.name, "Shop");
        assert!(body.id.is_none() && body.platform.is_none() && body.settings.is_none());

        assert!(
            serde_json::from_value::<CreateApplication>(
                serde_json::json!({ "name": "Shop", "orgId": "x" })
            )
            .is_err()
        );
    }
}
