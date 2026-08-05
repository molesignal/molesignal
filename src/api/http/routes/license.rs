// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `_sys` scoped immutable License history and active pointer.

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    api::AppState,
    app::iam::IamContext,
    domain::{
        iam::permission,
        license::{ActiveLicenseVersion, LicenseVersion},
    },
    infra::persistence::repositories::audit_events::AuditEvent,
    license::{DEFAULT_ROOT_PUBKEY, LicenseFile, SignedLicense},
    shared::{Error, LicenseGate, Result, ids::Id, time::TimestampMicros},
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/system/license", get(get_snapshot))
        .route("/system/license/versions", get(list_versions).post(upload))
        .route("/system/license/versions/{id}/activate", post(activate))
}

#[derive(Debug, Serialize)]
pub struct LicenseSnapshot {
    pub edition: &'static str,
    pub verified: bool,
    pub expired: bool,
    pub issued_to: String,
    pub features: Vec<String>,
    pub max_ingest_bytes_per_day: Option<u64>,
    pub expires_at_micros: Option<i64>,
    pub active_version_id: Option<String>,
}

fn snapshot_of(state: &AppState, active_version_id: Option<String>) -> LicenseSnapshot {
    let now = TimestampMicros::now().0;
    LicenseSnapshot {
        edition: state.platform.license.edition(),
        verified: state.platform.license.verified(),
        expired: state.platform.license.expired(now),
        issued_to: state.platform.license.issued_to().to_string(),
        features: state.platform.license.features(),
        max_ingest_bytes_per_day: state.platform.license.max_ingest_bytes_per_day(),
        expires_at_micros: state.platform.license.expires_at_micros(),
        active_version_id,
    }
}

#[permission("sys.licenses.read")]
async fn get_snapshot(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
) -> Result<Json<LicenseSnapshot>> {
    let active = state.platform.license_versions.active().await?;
    Ok(Json(snapshot_of(
        &state,
        active.map(|record| record.version.id.0),
    )))
}

#[derive(Debug, Serialize)]
struct LicenseVersionView {
    id: String,
    active: bool,
    summary: serde_json::Value,
    created_by: Option<String>,
    created_at_micros: i64,
    activated_by: Option<String>,
    activated_at_micros: Option<i64>,
}

fn version_view(
    version: LicenseVersion,
    active: Option<&ActiveLicenseVersion>,
) -> LicenseVersionView {
    let selected = active.filter(|candidate| candidate.version.id == version.id);
    LicenseVersionView {
        id: version.id.0,
        active: selected.is_some(),
        summary: version.summary,
        created_by: version.created_by.map(|id| id.0),
        created_at_micros: version.created_at.0,
        activated_by: selected
            .and_then(|candidate| candidate.activated_by.as_ref())
            .map(|id| id.0.clone()),
        activated_at_micros: selected.map(|candidate| candidate.activated_at.0),
    }
}

#[permission("sys.licenses.read")]
async fn list_versions(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
) -> Result<Json<Vec<LicenseVersionView>>> {
    let active = state.platform.license_versions.active().await?;
    let versions = state
        .platform
        .license_versions
        .list()
        .await?
        .into_iter()
        .map(|version| version_view(version, active.as_ref()))
        .collect();
    Ok(Json(versions))
}

#[derive(Debug, Deserialize)]
pub struct LicenseUploadRequest {
    pub payload_b64: String,
    pub signature_b64: String,
}

#[permission("sys.licenses.manage")]
async fn upload(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Json(request): Json<LicenseUploadRequest>,
) -> Result<(StatusCode, Json<LicenseSnapshot>)> {
    if request.payload_b64.is_empty() || request.signature_b64.is_empty() {
        return Err(Error::invalid("payload_b64 and signature_b64 are required"));
    }
    let file = LicenseFile {
        payload_b64: request.payload_b64,
        signature_b64: request.signature_b64,
    };
    let now = TimestampMicros::now();
    let verified = SignedLicense::verify_active(&file, &DEFAULT_ROOT_PUBKEY, now.0);
    if let Err(error) = &verified {
        record_license_audit(
            &state,
            &context,
            "license.upload",
            None,
            "verification_failed",
        )
        .await;
        return Err(Error::invalid(format!(
            "License verification failed: {error}"
        )));
    }
    let verified = verified.expect("checked above");
    let package = serde_json::to_value(&file)
        .map_err(|error| Error::internal(format!("serialize License package: {error}")))?;
    let digest = blake3::hash(&serde_json::to_vec(&package).unwrap_or_default())
        .to_hex()
        .to_string();
    let version = LicenseVersion {
        id: Id::new(),
        system_org_id: state.iam.system_org_id.clone(),
        signed_package: package,
        payload_digest: digest,
        summary: json!({
            "expires_at_micros": verified.expires_at_micros(),
            "feature_count": verified.features().len(),
            "max_ingest_bytes_per_day": verified.max_ingest_bytes_per_day(),
        }),
        created_by: Some(context.user_id.clone()),
        created_at: now,
    };
    let active = state
        .platform
        .license_versions
        .insert_and_activate(version, Some(&context.user_id))
        .await?;
    state
        .platform
        .license_holder
        .replace(std::sync::Arc::new(verified));
    record_license_audit(
        &state,
        &context,
        "license.upload_activate",
        Some(&active.version.id),
        "success",
    )
    .await;
    Ok((
        StatusCode::CREATED,
        Json(snapshot_of(&state, Some(active.version.id.0))),
    ))
}

#[permission("sys.licenses.manage")]
async fn activate(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<LicenseSnapshot>> {
    let version_id = Id(id);
    let version = state.platform.license_versions.get(&version_id).await?;
    let file: LicenseFile = serde_json::from_value(version.signed_package.clone())
        .map_err(|error| Error::invalid(format!("stored License package: {error}")))?;
    let verified =
        SignedLicense::verify_active(&file, &DEFAULT_ROOT_PUBKEY, TimestampMicros::now().0);
    if let Err(error) = &verified {
        record_license_audit(
            &state,
            &context,
            "license.activate",
            Some(&version_id),
            "verification_failed",
        )
        .await;
        return Err(Error::invalid(format!(
            "License verification failed: {error}"
        )));
    }
    let active = state
        .platform
        .license_versions
        .activate(&version_id, &context.user_id)
        .await?;
    state
        .platform
        .license_holder
        .replace(std::sync::Arc::new(verified.expect("checked above")));
    record_license_audit(
        &state,
        &context,
        "license.activate",
        Some(&active.version.id),
        "success",
    )
    .await;
    Ok(Json(snapshot_of(&state, Some(active.version.id.0))))
}

async fn record_license_audit(
    state: &AppState,
    context: &IamContext,
    action: &str,
    version_id: Option<&Id>,
    result: &str,
) {
    let _ = state
        .iam
        .audit_events
        .record(AuditEvent {
            id: Id::new(),
            org_id: state.iam.system_org_id.clone(),
            actor_kind: "user".into(),
            actor_id: context.user_id.0.clone(),
            action: action.into(),
            target_kind: Some("license_version".into()),
            target_id: version_id.map(|id| id.0.clone()),
            ip: None,
            user_agent: None,
            // No signed package, signature, credential, or secret reference.
            payload: json!({"result": result}),
            ts: TimestampMicros::now(),
        })
        .await;
}
