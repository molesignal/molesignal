// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::BTreeSet;

use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::super::OperationOutcome;
use crate::{
    agent::model::ApprovalRequest,
    api::AppState,
    app::iam::IamContext,
    infra::persistence::repositories::annotations::Annotation,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub(super) async fn execute(
    state: &AppState,
    ctx: &IamContext,
    approval: &ApprovalRequest,
) -> Result<OperationOutcome> {
    match approval.action.as_str() {
        "create_annotation" => create(state, ctx, approval).await,
        "update_annotation" => update(state, ctx, approval).await,
        "delete_annotation" => delete(state, ctx, approval).await,
        _ => unreachable!("annotation operation received unrelated action"),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateAnnotation {
    title: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    time_start_micros: i64,
    time_end_micros: i64,
    #[serde(default)]
    dashboard_id: Option<String>,
    #[serde(default)]
    stream_name: Option<String>,
}

async fn create(
    state: &AppState,
    ctx: &IamContext,
    approval: &ApprovalRequest,
) -> Result<OperationOutcome> {
    let parameters: CreateAnnotation = parse(&approval.parameters)?;
    let (title, tags) = validate_annotation(
        parameters.title,
        parameters.tags,
        parameters.time_start_micros,
        parameters.time_end_micros,
    )?;
    let annotation = state
        .storage
        .annotations
        .create(Annotation {
            id: Id(approval.target.clone()),
            org_id: ctx.org_id.clone(),
            title,
            description: parameters.description,
            tags,
            time_start: TimestampMicros(parameters.time_start_micros),
            time_end: TimestampMicros(parameters.time_end_micros),
            dashboard_id: optional_id(parameters.dashboard_id, "dashboard_id")?,
            stream_name: optional_text(parameters.stream_name, "stream_name")?,
            created_by: approval.requested_by.clone(),
            created_at: TimestampMicros::now(),
        })
        .await?;
    outcome("annotation created", annotation)
}

async fn update(
    state: &AppState,
    ctx: &IamContext,
    approval: &ApprovalRequest,
) -> Result<OperationOutcome> {
    let parameters = parameter_map(&approval.parameters)?;
    reject_unknown(
        parameters,
        &[
            "title",
            "description",
            "tags",
            "time_start_micros",
            "time_end_micros",
            "dashboard_id",
            "stream_name",
        ],
    )?;
    if parameters.is_empty() {
        return Err(Error::invalid(
            "update_annotation requires at least one field",
        ));
    }
    let mut annotation = state
        .storage
        .annotations
        .get(&ctx.org_id, &Id(approval.target.clone()))
        .await?;
    if let Some(value) = parameters.get("title") {
        annotation.title = parse(value)?;
    }
    if let Some(value) = parameters.get("description") {
        annotation.description = parse(value)?;
    }
    if let Some(value) = parameters.get("tags") {
        annotation.tags = parse(value)?;
    }
    if let Some(value) = parameters.get("time_start_micros") {
        annotation.time_start = TimestampMicros(parse(value)?);
    }
    if let Some(value) = parameters.get("time_end_micros") {
        annotation.time_end = TimestampMicros(parse(value)?);
    }
    if let Some(value) = parameters.get("dashboard_id") {
        annotation.dashboard_id = optional_id(parse(value)?, "dashboard_id")?;
    }
    if let Some(value) = parameters.get("stream_name") {
        annotation.stream_name = optional_text(parse(value)?, "stream_name")?;
    }
    let (title, tags) = validate_annotation(
        annotation.title,
        annotation.tags,
        annotation.time_start.0,
        annotation.time_end.0,
    )?;
    annotation.title = title;
    annotation.tags = tags;
    let annotation = state.storage.annotations.update(annotation).await?;
    outcome("annotation updated", annotation)
}

async fn delete(
    state: &AppState,
    ctx: &IamContext,
    approval: &ApprovalRequest,
) -> Result<OperationOutcome> {
    reject_unknown(parameter_map(&approval.parameters)?, &[])?;
    let id = Id(approval.target.clone());
    state.storage.annotations.get(&ctx.org_id, &id).await?;
    state.storage.annotations.delete(&ctx.org_id, &id).await?;
    Ok(OperationOutcome {
        summary: "annotation deleted".into(),
        verification: json!({"verified": true, "annotation_id": id, "deleted": true}),
    })
}

fn outcome(summary: &str, annotation: Annotation) -> Result<OperationOutcome> {
    Ok(OperationOutcome {
        summary: summary.into(),
        verification: json!({"verified": true, "annotation": annotation}),
    })
}

fn validate_annotation(
    title: String,
    tags: Vec<String>,
    start: i64,
    end: i64,
) -> Result<(String, Vec<String>)> {
    let title = title.trim().to_string();
    if title.is_empty() || title.chars().count() > 500 {
        return Err(Error::invalid(
            "title must contain between 1 and 500 characters",
        ));
    }
    if end < start {
        return Err(Error::invalid(
            "time_end_micros must be greater than or equal to time_start_micros",
        ));
    }
    let tags = tags
        .into_iter()
        .map(|tag| tag.trim().to_string())
        .filter(|tag| !tag.is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    if tags.len() > 64 || tags.iter().any(|tag| tag.len() > 128) {
        return Err(Error::invalid(
            "annotations support at most 64 tags of 128 bytes each",
        ));
    }
    Ok((title, tags))
}

fn optional_id(value: Option<String>, field: &str) -> Result<Option<Id>> {
    value
        .map(|value| {
            let value = value.trim();
            if value.is_empty() {
                Err(Error::invalid(format!("{field} must not be empty")))
            } else {
                Ok(Id(value.to_string()))
            }
        })
        .transpose()
}

fn optional_text(value: Option<String>, field: &str) -> Result<Option<String>> {
    value
        .map(|value| {
            let value = value.trim();
            if value.is_empty() || value.chars().count() > 500 {
                Err(Error::invalid(format!(
                    "{field} must contain between 1 and 500 characters"
                )))
            } else {
                Ok(value.to_string())
            }
        })
        .transpose()
}

fn parameter_map(value: &Value) -> Result<&Map<String, Value>> {
    value
        .as_object()
        .ok_or_else(|| Error::invalid("operation parameters must be an object"))
}

fn reject_unknown(parameters: &Map<String, Value>, allowed: &[&str]) -> Result<()> {
    if let Some(key) = parameters
        .keys()
        .find(|key| !allowed.contains(&key.as_str()))
    {
        Err(Error::invalid(format!(
            "unknown annotation operation parameter `{key}`"
        )))
    } else {
        Ok(())
    }
}

fn parse<T: serde::de::DeserializeOwned>(value: &Value) -> Result<T> {
    serde_json::from_value(value.clone())
        .map_err(|error| Error::invalid(format!("invalid operation parameters: {error}")))
}
