// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Upload and manage RUM artifacts used for JavaScript, Flutter, Android, and iOS stacks.

use axum::{
    Extension, Json, Router,
    extract::{DefaultBodyLimit, Multipart, Path, Query, State},
    routing::get,
};
use bytes::Bytes;
use object_store::{ObjectStoreExt, path::Path as ObjectPath};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    api::AppState,
    app::iam::IamContext,
    domain::{
        iam::permission,
        rum::{
            DebugArtifactKind, DebugArtifactMeta, normalize_architecture, normalize_debug_id,
            validate_application_id,
        },
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

const MAX_ARTIFACT_BYTES: usize = 50 * 1024 * 1024;
const MAX_MULTIPART_BYTES: usize = MAX_ARTIFACT_BYTES + 1024 * 1024;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/debug-artifacts",
            get(list)
                .post(upload)
                .layer(DefaultBodyLimit::max(MAX_MULTIPART_BYTES)),
        )
        .route("/debug-artifacts/{id}", axum::routing::delete(delete))
}

#[derive(Debug, Deserialize)]
pub struct ListParams {
    pub application_id: Option<String>,
    pub service: Option<String>,
    pub kind: Option<String>,
    pub platform: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ArtifactResponse {
    pub id: String,
    pub application_id: String,
    pub service: String,
    pub release: String,
    pub kind: String,
    pub platform: String,
    pub architecture: String,
    pub debug_id: String,
    pub filename: String,
    pub size_bytes: u64,
    pub checksum_sha256: String,
    pub uploaded_at_micros: i64,
}

fn to_response(artifact: DebugArtifactMeta) -> ArtifactResponse {
    ArtifactResponse {
        id: artifact.id.0,
        application_id: artifact.application_id,
        service: artifact.service,
        release: artifact.release,
        kind: artifact.kind.as_str().to_string(),
        platform: artifact.platform,
        architecture: artifact.architecture,
        debug_id: artifact.debug_id,
        filename: artifact.filename,
        size_bytes: artifact.size_bytes,
        checksum_sha256: artifact.checksum_sha256,
        uploaded_at_micros: artifact.uploaded_at.0,
    }
}

#[permission("streams.read")]
async fn list(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Query(params): Query<ListParams>,
) -> Result<Json<Vec<ArtifactResponse>>> {
    let kind = params.kind.as_deref().map(parse_kind).transpose()?;
    let artifacts = state
        .storage
        .debug_artifacts
        .list(
            &context.org_id,
            normalized_filter(params.application_id.as_deref()),
            normalized_filter(params.service.as_deref()),
            kind,
            normalized_filter(params.platform.as_deref()),
        )
        .await?;
    Ok(Json(artifacts.into_iter().map(to_response).collect()))
}

#[permission("streams.configure")]
async fn upload(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    mut multipart: Multipart,
) -> Result<Json<ArtifactResponse>> {
    let mut fields = UploadFields::default();
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|error| Error::invalid(format!("multipart: {error}")))?
    {
        let name = field.name().unwrap_or_default().to_string();
        if name == "file" {
            fields.filename = field.file_name().map(safe_filename);
            fields.bytes = Some(
                field
                    .bytes()
                    .await
                    .map_err(|error| Error::invalid(format!("artifact file: {error}")))?,
            );
        } else {
            let value = field
                .text()
                .await
                .map_err(|error| Error::invalid(format!("field {name}: {error}")))?;
            fields.set_text(&name, value);
        }
    }

    let artifact = fields.into_artifact(&context.org_id)?;
    let bytes = artifact.bytes;
    let validation_bytes = bytes.clone();
    let validation_filename = artifact.meta.filename.clone();
    let validation_kind = artifact.meta.kind;
    tokio::task::spawn_blocking(move || {
        crate::infra::rum::symbolication::validate_artifact(
            validation_kind,
            &validation_filename,
            &validation_bytes,
        )
    })
    .await
    .map_err(|error| Error::internal(format!("debug artifact validation task: {error}")))??;
    let path = ObjectPath::parse(&artifact.meta.object_key)
        .map_err(|error| Error::internal(format!("debug artifact object path: {error}")))?;
    state
        .storage
        .object_store
        .put(&path, bytes.into())
        .await
        .map_err(|error| Error::internal(format!("debug artifact upload: {error}")))?;
    let object_key = artifact.meta.object_key.clone();
    let upsert = match state.storage.debug_artifacts.create(artifact.meta).await {
        Ok(saved) => saved,
        Err(error) => {
            let _ = state.storage.object_store.delete(&path).await;
            return Err(error);
        }
    };
    let saved = upsert.artifact;
    if let Some(previous_key) = upsert
        .replaced_object_key
        .filter(|previous_key| previous_key != &saved.object_key)
        && let Ok(previous_path) = ObjectPath::parse(&previous_key)
        && let Err(error) = state.storage.object_store.delete(&previous_path).await
        && !matches!(error, object_store::Error::NotFound { .. })
    {
        tracing::warn!(artifact_id = %saved.id.0, %error, "replaced debug artifact cleanup failed");
    }
    debug_assert_eq!(saved.object_key, object_key);
    Ok(Json(to_response(saved)))
}

#[permission("streams.configure")]
async fn delete(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    let artifact = state
        .storage
        .debug_artifacts
        .delete(&context.org_id, &Id(id))
        .await?
        .ok_or_else(|| Error::not_found("debug artifact"))?;
    if let Ok(path) = ObjectPath::parse(&artifact.object_key)
        && let Err(error) = state.storage.object_store.delete(&path).await
        && !matches!(error, object_store::Error::NotFound { .. })
    {
        tracing::warn!(artifact_id = %artifact.id.0, %error, "debug artifact object cleanup failed");
    }
    Ok(Json(serde_json::json!({"deleted": true})))
}

#[derive(Default)]
struct UploadFields {
    application_id: Option<String>,
    service: Option<String>,
    release: Option<String>,
    kind: Option<String>,
    platform: Option<String>,
    architecture: Option<String>,
    debug_id: Option<String>,
    filename: Option<String>,
    bytes: Option<Bytes>,
}

impl UploadFields {
    fn set_text(&mut self, name: &str, value: String) {
        match name {
            "application_id" => self.application_id = Some(value),
            "service" => self.service = Some(value),
            "release" => self.release = Some(value),
            "kind" => self.kind = Some(value),
            "platform" => self.platform = Some(value),
            "architecture" => self.architecture = Some(value),
            "debug_id" => self.debug_id = Some(value),
            _ => {}
        }
    }

    fn into_artifact(self, org_id: &Id) -> Result<UploadArtifact> {
        let application_id = required(self.application_id, "application_id", 128)?;
        let application_id = validate_application_id(&application_id)?.to_string();
        let service = required(self.service, "service", 255)?;
        let release = required(self.release, "release", 64)?;
        let kind = parse_kind(&required(self.kind, "kind", 32)?)?;
        let platform = required(self.platform, "platform", 16)?.to_ascii_lowercase();
        let architecture =
            normalize_architecture(&optional(self.architecture, "architecture", 32)?);
        let debug_id = normalize_debug_id(&optional(self.debug_id, "debug_id", 128)?);
        let filename = self
            .filename
            .filter(|value| !value.is_empty())
            .ok_or_else(|| Error::invalid("file field with filename is required"))?;
        validate_kind_platform(kind, &platform)?;
        let bytes = self
            .bytes
            .ok_or_else(|| Error::invalid("file field is required"))?;
        if bytes.is_empty() {
            return Err(Error::invalid("debug artifact must not be empty"));
        }
        if bytes.len() > MAX_ARTIFACT_BYTES {
            return Err(Error::invalid("debug artifact exceeds 50 MiB"));
        }
        let checksum_sha256 = hex::encode(Sha256::digest(&bytes));
        let id = Id::new();
        let object_key = build_object_key(
            org_id,
            &id,
            &application_id,
            &service,
            &release,
            kind,
            &platform,
            &architecture,
            &debug_id,
            &filename,
        );
        Ok(UploadArtifact {
            meta: DebugArtifactMeta {
                id,
                org_id: org_id.clone(),
                application_id,
                service,
                release,
                kind,
                platform,
                architecture,
                debug_id,
                filename,
                object_key,
                size_bytes: bytes.len() as u64,
                checksum_sha256,
                uploaded_at: TimestampMicros::now(),
            },
            bytes,
        })
    }
}

struct UploadArtifact {
    meta: DebugArtifactMeta,
    bytes: Bytes,
}

fn parse_kind(value: &str) -> Result<DebugArtifactKind> {
    DebugArtifactKind::parse(value.trim())
        .ok_or_else(|| Error::invalid(format!("unsupported debug artifact kind: {value}")))
}

fn validate_kind_platform(kind: DebugArtifactKind, platform: &str) -> Result<()> {
    let valid = match kind {
        DebugArtifactKind::JavascriptSourcemap => matches!(platform, "web" | "flutter"),
        DebugArtifactKind::FlutterSymbols => matches!(platform, "android" | "ios"),
        DebugArtifactKind::AndroidMapping | DebugArtifactKind::AndroidNativeSymbols => {
            platform == "android"
        }
        DebugArtifactKind::AppleDsym => platform == "ios",
    };
    if valid {
        Ok(())
    } else {
        Err(Error::invalid(format!(
            "artifact kind {} is not valid for platform {platform}",
            kind.as_str()
        )))
    }
}

fn required(value: Option<String>, field: &str, max: usize) -> Result<String> {
    let value = value.unwrap_or_default().trim().to_string();
    if value.is_empty() {
        return Err(Error::invalid(format!("field {field} is required")));
    }
    validate_length(&value, field, max)?;
    Ok(value)
}

fn optional(value: Option<String>, field: &str, max: usize) -> Result<String> {
    let value = value.unwrap_or_default().trim().to_string();
    validate_length(&value, field, max)?;
    Ok(value)
}

fn validate_length(value: &str, field: &str, max: usize) -> Result<()> {
    if value.len() <= max {
        Ok(())
    } else {
        Err(Error::invalid(format!("field {field} exceeds {max} bytes")))
    }
}

fn normalized_filter(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn safe_filename(value: &str) -> String {
    safe_segment(value.rsplit(['/', '\\']).next().unwrap_or_default(), 255)
}

fn safe_segment(value: &str, max: usize) -> String {
    value
        .chars()
        .take(max)
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn build_object_key(
    org_id: &Id,
    artifact_id: &Id,
    application_id: &str,
    service: &str,
    release: &str,
    kind: DebugArtifactKind,
    platform: &str,
    architecture: &str,
    debug_id: &str,
    filename: &str,
) -> String {
    let identity = [
        application_id,
        service,
        release,
        kind.as_str(),
        platform,
        architecture,
        debug_id,
        filename,
    ]
    .join("\0");
    let identity_hash = hex::encode(Sha256::digest(identity.as_bytes()));
    format!(
        "debug-artifacts/{}/{identity_hash}/{}/{filename}",
        safe_segment(org_id.as_str(), 64),
        safe_segment(artifact_id.as_str(), 64),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn object_keys_do_not_embed_user_controlled_paths() {
        let key = build_object_key(
            &Id::from_string("org/one"),
            &Id::from_string("artifact/one"),
            "../../application",
            "mobile/app",
            "1.0.0",
            DebugArtifactKind::FlutterSymbols,
            "android",
            "arm64",
            "debug-id",
            &safe_filename("../../app.android-arm64.symbols"),
        );
        assert!(key.starts_with("debug-artifacts/org_one/"));
        assert!(key.ends_with("/artifact_one/app.android-arm64.symbols"));
        assert!(!key.contains("../"));
    }

    #[test]
    fn platform_and_kind_must_match() {
        assert!(validate_kind_platform(DebugArtifactKind::AndroidMapping, "android").is_ok());
        assert!(validate_kind_platform(DebugArtifactKind::AppleDsym, "android").is_err());
    }
}
