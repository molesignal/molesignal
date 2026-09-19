// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::{collections::HashSet, time::Duration};

use anyhow::{Context as _, Result, anyhow, bail};
use futures::{StreamExt as _, stream};
use reqwest::{
    Client, Url,
    header::{HeaderMap, HeaderName, HeaderValue},
};
use sha2::{Digest as _, Sha256};

use super::{ArtifactPayload, now_micros};
use crate::protocol::v1::{self as wire, Artifact};

const UPLOAD_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_UPLOAD_CONCURRENCY: usize = 4;

pub(super) struct UploadOutcome {
    pub captured_count: usize,
    pub receipts: Vec<Artifact>,
    pub error: Option<String>,
}

pub(super) async fn upload(
    task: &wire::ProbeTask,
    captured: Vec<ArtifactPayload>,
) -> UploadOutcome {
    let captured_count = captured.len();
    if captured.is_empty() {
        return UploadOutcome {
            captured_count,
            receipts: Vec::new(),
            error: None,
        };
    }
    if task.artifact_uploads.is_empty() {
        return UploadOutcome {
            captured_count,
            receipts: Vec::new(),
            error: Some("Probe task did not include Artifact upload targets".into()),
        };
    }

    let client = Client::builder()
        .timeout(UPLOAD_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .build();
    let mut receipts = Vec::new();
    let mut used = HashSet::new();
    let mut errors = Vec::new();
    match client {
        Ok(client) => {
            let mut pending = Vec::with_capacity(captured.len());
            for artifact in captured {
                let target = task.artifact_uploads.iter().find(|target| {
                    !used.contains(&target.artifact_id)
                        && target.kind == artifact.kind
                        && (target.name.is_empty() || target.name == artifact.name)
                });
                let Some(target) = target else {
                    errors.push(format!(
                        "no upload target for {} `{}`",
                        artifact.kind, artifact.name
                    ));
                    continue;
                };
                used.insert(target.artifact_id.clone());
                pending.push((target.clone(), artifact));
            }
            let mut uploads = stream::iter(pending.into_iter().map(|(target, artifact)| {
                let client = client.clone();
                async move { upload_one(&client, &target, artifact).await }
            }))
            .buffer_unordered(MAX_UPLOAD_CONCURRENCY);
            while let Some(uploaded) = uploads.next().await {
                match uploaded {
                    Ok(receipt) => receipts.push(receipt),
                    Err(error) => errors.push(error.to_string()),
                }
            }
        }
        Err(error) => errors.push(format!("create Artifact upload client: {error}")),
    }

    UploadOutcome {
        captured_count,
        receipts,
        error: (!errors.is_empty()).then(|| truncate(errors.join("; "), 4096)),
    }
}

async fn upload_one(
    client: &Client,
    target: &wire::ArtifactUploadTarget,
    artifact: ArtifactPayload,
) -> Result<Artifact> {
    if target.artifact_id.is_empty() || target.kind.is_empty() {
        bail!("invalid Artifact upload target");
    }
    if target.expires_at_micros <= now_micros() {
        bail!("Artifact upload target `{}` expired", target.artifact_id);
    }
    if target.max_bytes == 0 || artifact.bytes.len() as u64 > target.max_bytes {
        bail!("Artifact `{}` exceeds its upload limit", artifact.name);
    }
    let url = Url::parse(&target.upload_url).context("parse Artifact upload URL")?;
    if !matches!(url.scheme(), "http" | "https") {
        bail!("Artifact upload URL must use http or https");
    }
    let mut headers = target
        .upload_headers
        .iter()
        .map(|(name, value)| {
            Ok((
                HeaderName::from_bytes(name.as_bytes())?,
                HeaderValue::from_str(value)?,
            ))
        })
        .collect::<Result<HeaderMap>>()?;
    let content_length = artifact.bytes.len() as u64;
    let sha256 = Sha256::digest(&artifact.bytes).to_vec();
    headers.insert(
        HeaderName::from_static("x-molesignal-content-sha256"),
        HeaderValue::from_str(&hex::encode(&sha256))?,
    );
    let response = client
        .put(url)
        .headers(headers)
        .body(artifact.bytes)
        .send()
        .await
        .with_context(|| format!("upload Artifact `{}`", artifact.name))?;
    if !response.status().is_success() {
        return Err(anyhow!(
            "upload Artifact `{}` returned HTTP {}",
            artifact.name,
            response.status()
        ));
    }
    Ok(Artifact {
        artifact_id: target.artifact_id.clone(),
        kind: target.kind.clone(),
        content_length,
        sha256: sha256.into(),
        uploaded: true,
        name: artifact.name,
    })
}

fn truncate(mut value: String, max: usize) -> String {
    if value.len() <= max {
        return value;
    }
    value.truncate(max);
    value
}
