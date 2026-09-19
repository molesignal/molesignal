// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::HashMap;

use object_store::{ObjectStoreExt, path::Path as ObjectPath};
use serde_json::{Map, Value};

use crate::{
    api::AppState,
    app::iam::IamContext,
    domain::rum::{DebugArtifactKind, DebugArtifactLookup, DebugArtifactMeta},
    infra::rum::symbolication::{OriginalFrame, decode_artifact},
    shared::{Error, Result},
};

mod frame;
mod prepared;

use frame::{EventMetadata, FramePlan};
use prepared::PreparedArtifact;

const MAX_SYMBOLICATED_FRAMES_PER_EVENT: usize = 256;
const MAX_PREPARED_ARTIFACTS_PER_REQUEST: usize = 16;

pub(super) async fn translate_body(
    state: &AppState,
    context: &IamContext,
    mut body: Value,
) -> Value {
    let events = match &mut body {
        Value::Array(events) => events.as_mut_slice(),
        Value::Object(_) => std::slice::from_mut(&mut body),
        _ => return body,
    };
    let mut artifacts = HashMap::new();
    for event in events {
        if let Err(error) = translate_event(state, context, event, &mut artifacts).await {
            tracing::debug!(%error, "RUM debug artifact symbolication skipped");
        }
    }
    body
}

async fn translate_event(
    state: &AppState,
    context: &IamContext,
    event: &mut Value,
    artifacts: &mut HashMap<String, PreparedArtifact>,
) -> Result<()> {
    let metadata = EventMetadata::from_event(event)?;
    let Some(stack_path) = stack_path(event) else {
        return Ok(());
    };
    let frames = event
        .pointer_mut(stack_path)
        .and_then(Value::as_array_mut)
        .expect("stack path selected from an array");
    let truncated = frames.len() > MAX_SYMBOLICATED_FRAMES_PER_EVENT;
    let mut attempted = 0_u64;
    let mut translated = 0_u64;
    for frame in frames.iter_mut().take(MAX_SYMBOLICATED_FRAMES_PER_EVENT) {
        let Some(plan) = FramePlan::from_frame(frame, &metadata) else {
            continue;
        };
        attempted += 1;
        match resolve_frame(state, context, &metadata, &plan, artifacts).await {
            Ok(Some(resolved)) => {
                apply_original_frame(frame, resolved);
                translated += 1;
            }
            Ok(None) => {}
            Err(error) => {
                tracing::debug!(%error, kind = plan.kind.as_str(), "RUM frame symbolication skipped");
            }
        }
    }
    if attempted > 0
        && let Some(object) = event.as_object_mut()
    {
        let status = if translated == attempted && !truncated {
            "completed"
        } else if translated > 0 {
            "partial"
        } else {
            "missing"
        };
        object.insert(
            "symbolication".into(),
            serde_json::json!({
                "status": status,
                "translated_frames": translated,
                "attempted_frames": attempted,
                "truncated": truncated,
            }),
        );
    }
    Ok(())
}

struct ResolvedFrame {
    original: OriginalFrame,
    original_class: Option<String>,
    artifact_id: String,
    artifact_kind: DebugArtifactKind,
}

async fn resolve_frame(
    state: &AppState,
    context: &IamContext,
    event: &EventMetadata,
    plan: &FramePlan,
    artifacts: &mut HashMap<String, PreparedArtifact>,
) -> Result<Option<ResolvedFrame>> {
    let platform = artifact_platform(event, plan.kind);
    let mut artifact = find_artifact(
        state,
        context,
        event,
        plan,
        platform,
        plan.filename.as_deref(),
    )
    .await?;
    if artifact.is_none()
        && let Some(filename) = plan.filename.as_deref()
        && !filename.to_ascii_lowercase().ends_with(".gz")
    {
        artifact = find_artifact(
            state,
            context,
            event,
            plan,
            platform,
            Some(&format!("{filename}.gz")),
        )
        .await?;
    }
    if artifact.is_none()
        && plan.filename.is_some()
        && plan.kind != DebugArtifactKind::JavascriptSourcemap
    {
        // Flutter split-debug-info files do not normally share the runtime
        // module name (for example libapp.so vs app.android-arm64.symbols).
        // The repository only returns this fallback when the remaining
        // application/build identity selects one artifact unambiguously.
        artifact = find_artifact(state, context, event, plan, platform, None).await?;
    }
    let Some(artifact) = artifact else {
        return Ok(None);
    };
    let prepared = load_artifact(state, &artifact, artifacts).await?;
    let Some((original, original_class)) = prepared.translate(plan)? else {
        return Ok(None);
    };
    if original.file.is_none() && original.function.is_none() && original.line.is_none() {
        return Ok(None);
    }
    Ok(Some(ResolvedFrame {
        original,
        original_class,
        artifact_id: artifact.id.0,
        artifact_kind: artifact.kind,
    }))
}

async fn find_artifact(
    state: &AppState,
    context: &IamContext,
    event: &EventMetadata,
    plan: &FramePlan,
    platform: &str,
    filename: Option<&str>,
) -> Result<Option<DebugArtifactMeta>> {
    state
        .storage
        .debug_artifacts
        .find_best(
            &context.org_id,
            &DebugArtifactLookup {
                application_id: &event.application_id,
                service: &event.service,
                release: &event.release,
                kind: plan.kind,
                platform: Some(platform),
                architecture: event.architecture.as_deref(),
                debug_id: plan.debug_id.as_deref().or(event.debug_id.as_deref()),
                filename,
            },
        )
        .await
}

async fn load_artifact<'a>(
    state: &AppState,
    artifact: &DebugArtifactMeta,
    artifacts: &'a mut HashMap<String, PreparedArtifact>,
) -> Result<&'a PreparedArtifact> {
    if !artifacts.contains_key(&artifact.object_key) {
        if artifacts.len() >= MAX_PREPARED_ARTIFACTS_PER_REQUEST {
            return Err(Error::resource_exhausted(format!(
                "RUM request references more than {MAX_PREPARED_ARTIFACTS_PER_REQUEST} debug artifacts"
            )));
        }
        let path = ObjectPath::parse(&artifact.object_key)
            .map_err(|error| Error::internal(format!("debug artifact object path: {error}")))?;
        let stored = state
            .storage
            .object_store
            .get(&path)
            .await
            .map_err(|error| Error::internal(format!("debug artifact read: {error}")))?
            .bytes()
            .await
            .map_err(|error| Error::internal(format!("debug artifact bytes: {error}")))?;
        let filename = artifact.filename.clone();
        let kind = artifact.kind;
        let prepared = tokio::task::spawn_blocking(move || {
            let decoded = decode_artifact(&filename, &stored)?;
            PreparedArtifact::parse(kind, &decoded)
        })
        .await
        .map_err(|error| Error::internal(format!("debug artifact parse task: {error}")))??;
        artifacts.insert(artifact.object_key.clone(), prepared);
    }
    artifacts
        .get(&artifact.object_key)
        .ok_or_else(|| Error::internal("prepared debug artifact cache entry is missing"))
}

fn apply_original_frame(frame: &mut Value, resolved: ResolvedFrame) {
    let Some(object) = frame.as_object_mut() else {
        return;
    };
    insert_optional_string(object, "original_file", resolved.original.file);
    insert_optional_string(object, "original_function", resolved.original.function);
    insert_optional_string(object, "original_class", resolved.original_class);
    insert_optional_number(object, "original_line", resolved.original.line);
    insert_optional_number(object, "original_column", resolved.original.column);
    object.insert(
        "debug_artifact_id".into(),
        Value::String(resolved.artifact_id),
    );
    object.insert(
        "debug_artifact_kind".into(),
        Value::String(resolved.artifact_kind.as_str().to_string()),
    );
}

fn insert_optional_string(object: &mut Map<String, Value>, key: &str, value: Option<String>) {
    if let Some(value) = value {
        object.insert(key.into(), Value::String(value));
    }
}

fn insert_optional_number(object: &mut Map<String, Value>, key: &str, value: Option<u32>) {
    if let Some(value) = value {
        object.insert(key.into(), Value::Number(value.into()));
    }
}

fn stack_path(event: &Value) -> Option<&'static str> {
    ["/error/stack", "/error/stack_frames", "/stack"]
        .into_iter()
        .find(|path| event.pointer(path).is_some_and(Value::is_array))
}

fn artifact_platform(event: &EventMetadata, kind: DebugArtifactKind) -> &str {
    if kind == DebugArtifactKind::JavascriptSourcemap && event.flutter {
        "flutter"
    } else if kind == DebugArtifactKind::JavascriptSourcemap {
        "web"
    } else {
        &event.platform
    }
}
