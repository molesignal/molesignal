// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use axum::{
    Extension, Router,
    body::{Body, Bytes},
    extract::{DefaultBodyLimit, Path, State},
    http::{
        HeaderMap, StatusCode,
        header::{AUTHORIZATION, CACHE_CONTROL, CONTENT_LENGTH, CONTENT_TYPE},
    },
    response::Response,
    routing::{get, put},
};
use object_store::{
    ObjectStoreExt as _, PutMode, PutOptions, PutPayload, path::Path as ObjectPath,
};
use sha2::{Digest as _, Sha256};

use crate::{
    api::AppState,
    app::{
        iam::IamContext,
        synthetics::{MAX_HAR_BYTES, artifact_object_key, artifact_targets},
    },
    domain::iam::permission,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

const CONTENT_SHA256: &str = "x-molesignal-content-sha256";

pub(super) fn upload_routes() -> Router<AppState> {
    Router::new().route(
        "/api/v1/synthetics/artifacts/{task_id}/{artifact_id}",
        put(upload).layer(DefaultBodyLimit::max(MAX_HAR_BYTES as usize)),
    )
}

pub(super) fn download_routes() -> Router<AppState> {
    Router::new().route(
        "/synthetics/results/{result_id}/artifacts/{artifact_id}",
        get(download),
    )
}

async fn upload(
    State(state): State<AppState>,
    Path((task_id, artifact_id)): Path<(String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<StatusCode> {
    let lease_token = bearer_token(&headers)?;
    let control = state
        .probe_control
        .as_ref()
        .ok_or_else(|| Error::unavailable("Probe control plane is unavailable"))?;
    let task = control
        .verify_artifact_lease(&Id::from_string(task_id), lease_token)
        .await?;
    let target = artifact_targets(&task, lease_token)
        .into_iter()
        .find(|target| target.id == artifact_id)
        .ok_or_else(|| Error::unauthorized("invalid Probe Artifact target"))?;
    if target.expires_at <= TimestampMicros::now() {
        return Err(Error::unauthorized("Probe Artifact target expired"));
    }
    if body.is_empty() || body.len() as u64 > target.max_bytes {
        return Err(Error::payload_too_large(
            "Probe Artifact exceeds its upload limit",
        ));
    }
    let content_type = headers
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    if content_type != target.content_type {
        return Err(Error::invalid(
            "Probe Artifact content type does not match its target",
        ));
    }
    let expected_sha = headers
        .get(CONTENT_SHA256)
        .and_then(|value| value.to_str().ok())
        .filter(|value| value.len() == 64)
        .ok_or_else(|| Error::invalid("Probe Artifact SHA-256 header is required"))?;
    let actual_sha = hex::encode(Sha256::digest(&body));
    if !constant_time_eq(expected_sha.as_bytes(), actual_sha.as_bytes()) {
        return Err(Error::invalid(
            "Probe Artifact SHA-256 does not match its content",
        ));
    }

    let object_key = artifact_object_key(&task, &artifact_id);
    let path = ObjectPath::parse(&object_key)
        .map_err(|error| Error::internal(format!("invalid Probe Artifact object key: {error}")))?;
    let stored = state
        .storage
        .object_store
        .put_opts(
            &path,
            PutPayload::from(body.clone()),
            PutOptions {
                mode: PutMode::Create,
                ..PutOptions::default()
            },
        )
        .await;
    match stored {
        Ok(_) => Ok(StatusCode::NO_CONTENT),
        Err(object_store::Error::AlreadyExists { .. }) => {
            let existing = state
                .storage
                .object_store
                .get(&path)
                .await
                .map_err(|error| Error::internal(format!("read existing Probe Artifact: {error}")))?
                .bytes()
                .await
                .map_err(|error| {
                    Error::internal(format!("read existing Probe Artifact body: {error}"))
                })?;
            let existing_sha = hex::encode(Sha256::digest(&existing));
            if existing.len() == body.len()
                && constant_time_eq(existing_sha.as_bytes(), actual_sha.as_bytes())
            {
                Ok(StatusCode::NO_CONTENT)
            } else {
                Err(Error::conflict(
                    "Probe Artifact target already contains other content",
                ))
            }
        }
        Err(error) => Err(Error::internal(format!("store Probe Artifact: {error}"))),
    }
}

#[permission("synthetics.read")]
async fn download(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path((result_id, artifact_id)): Path<(String, String)>,
) -> Result<Response> {
    let result = state
        .synthetics
        .get_result(&context.org_id, &Id::from_string(result_id))
        .await?;
    let artifact = result
        .artifacts
        .into_iter()
        .find(|artifact| artifact.id.as_str() == artifact_id)
        .ok_or_else(|| Error::not_found("synthetic Artifact not found"))?;
    if artifact.expires_at <= TimestampMicros::now() {
        return Err(Error::not_found("synthetic Artifact expired"));
    }
    let path = ObjectPath::parse(&artifact.object_key)
        .map_err(|error| Error::internal(format!("invalid Probe Artifact object key: {error}")))?;
    let body = state
        .storage
        .object_store
        .get(&path)
        .await
        .map_err(|error| match error {
            object_store::Error::NotFound { .. } => {
                Error::not_found("synthetic Artifact content not found")
            }
            error => Error::internal(format!("read synthetic Artifact: {error}")),
        })?
        .bytes()
        .await
        .map_err(|error| Error::internal(format!("read synthetic Artifact body: {error}")))?;
    if body.len() as u64 != artifact.content_length
        || !constant_time_eq(
            hex::encode(Sha256::digest(&body)).as_bytes(),
            artifact.sha256.as_bytes(),
        )
    {
        return Err(Error::internal("synthetic Artifact integrity check failed"));
    }
    Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, artifact.content_type)
        .header(CONTENT_LENGTH, body.len().to_string())
        .header(CACHE_CONTROL, "private, no-store")
        .body(Body::from(body))
        .map_err(|error| Error::internal(format!("build synthetic Artifact response: {error}")))
}

fn bearer_token(headers: &HeaderMap) -> Result<&str> {
    headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .filter(|value| (32..=256).contains(&value.len()))
        .ok_or_else(|| Error::unauthorized("invalid Probe Artifact lease token"))
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .fold(0_u8, |difference, (left, right)| {
                difference | (left ^ right)
            })
            == 0
}

#[cfg(test)]
mod tests {
    use super::constant_time_eq;

    #[test]
    fn digest_comparison_requires_every_byte() {
        assert!(constant_time_eq(b"same", b"same"));
        assert!(!constant_time_eq(b"same", b"some"));
        assert!(!constant_time_eq(b"same", b"same-longer"));
    }
}
