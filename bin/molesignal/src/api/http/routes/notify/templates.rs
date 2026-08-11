// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Notify 模板 CRUD。

use std::collections::BTreeSet;

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    routing::get,
};
use serde::{Deserialize, Serialize};

use crate::{
    api::{
        AppState,
        http::federation::{delete_payload, emit_cud},
    },
    app::{iam::IamContext, notify::validate_notify_template_body},
    domain::{
        federation::{CudAction, ResourceKind},
        iam::permission,
        notify::{
            preference::NotifyCategory,
            template::{
                NotifyTemplateField, NotifyTemplatePreset, notify_template_field_catalog,
                notify_template_preset_catalog,
            },
        },
    },
    infra::persistence::repositories::notify::NotifyTemplateRecord,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/notify/templates", get(list).post(create))
        .route("/notify/template-fields", get(list_fields))
        .route(
            "/notify/templates/{id}",
            get(get_one).put(update).delete(delete),
        )
}

#[derive(Debug, Deserialize)]
struct WriteRequest {
    name: String,
    body: String,
    #[serde(default)]
    format: Option<String>,
    #[serde(default = "default_template_category")]
    category: NotifyCategory,
}

const ALLOWED_FORMATS: &[&str] = &["text", "markdown", "html"];

const fn default_template_category() -> NotifyCategory {
    NotifyCategory::Alert
}

fn validate_request(request: &WriteRequest) -> Result<String> {
    if request.name.trim().is_empty() {
        return Err(Error::invalid("name must not be empty"));
    }
    validate_notify_template_body(&request.body)?;
    let format = request.format.as_deref().unwrap_or("text");
    if !ALLOWED_FORMATS.contains(&format) {
        return Err(Error::invalid(
            "format must be one of: text | markdown | html",
        ));
    }
    Ok(format.to_string())
}

#[derive(Debug, Serialize)]
struct TemplateFieldsResponse {
    fields: Vec<NotifyTemplateField>,
    presets: Vec<NotifyTemplatePreset>,
    label_keys: Vec<String>,
    annotation_keys: Vec<String>,
}

#[permission("alerts.read")]
async fn list_fields(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
) -> Result<Json<TemplateFieldsResponse>> {
    let mut label_keys = BTreeSet::new();
    let mut annotation_keys = BTreeSet::new();
    if let Ok(rules) = state.alerting.service.list_rules(&context.org_id).await {
        for rule in rules {
            label_keys.extend(rule.labels.keys().cloned());
            annotation_keys.extend(rule.annotations.keys().cloned());
        }
    }
    if let Ok(incidents) = state
        .alerting
        .service
        .list_incidents_active(&context.org_id)
        .await
    {
        for incident in incidents {
            label_keys.extend(incident.labels.keys().cloned());
            annotation_keys.extend(incident.annotations.keys().cloned());
        }
    }
    Ok(Json(TemplateFieldsResponse {
        fields: notify_template_field_catalog(),
        presets: notify_template_preset_catalog(),
        label_keys: label_keys.into_iter().collect(),
        annotation_keys: annotation_keys.into_iter().collect(),
    }))
}

#[permission("alerts.read")]
async fn list(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
) -> Result<Json<Vec<NotifyTemplateRecord>>> {
    Ok(Json(state.alerting.templates.list(&context.org_id).await?))
}

#[permission("alerts.manage")]
async fn create(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Json(request): Json<WriteRequest>,
) -> Result<Json<NotifyTemplateRecord>> {
    let format = validate_request(&request)?;
    let existing = state.alerting.templates.list(&context.org_id).await?;
    if existing
        .iter()
        .any(|template| template.name == request.name)
    {
        return Err(Error::conflict("notify template name already exists"));
    }
    let now = TimestampMicros::now();
    let saved = state
        .alerting
        .templates
        .create(NotifyTemplateRecord {
            id: Id::new(),
            organization_id: context.org_id.clone(),
            name: request.name,
            body: request.body,
            format,
            category: request.category,
            created_at: now,
            updated_at: now,
        })
        .await?;
    emit_cud(
        &state,
        &context.org_id,
        ResourceKind::NotifyTemplate,
        CudAction::Created,
        &saved.id.0,
        &saved,
    )
    .await;
    Ok(Json(saved))
}

#[permission("alerts.read")]
async fn get_one(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<NotifyTemplateRecord>> {
    Ok(Json(
        state
            .alerting
            .templates
            .get(&context.org_id, &Id::from_string(id))
            .await?,
    ))
}

#[permission("alerts.manage")]
async fn update(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(id): Path<String>,
    Json(request): Json<WriteRequest>,
) -> Result<Json<NotifyTemplateRecord>> {
    let format = validate_request(&request)?;
    let existing = state
        .alerting
        .templates
        .get(&context.org_id, &Id::from_string(id))
        .await?;
    if state
        .alerting
        .templates
        .list(&context.org_id)
        .await?
        .iter()
        .any(|template| template.id != existing.id && template.name == request.name)
    {
        return Err(Error::conflict("notify template name already exists"));
    }
    let saved = state
        .alerting
        .templates
        .update(NotifyTemplateRecord {
            id: existing.id,
            organization_id: context.org_id.clone(),
            name: request.name,
            body: request.body,
            format,
            category: request.category,
            created_at: existing.created_at,
            updated_at: TimestampMicros::now(),
        })
        .await?;
    emit_cud(
        &state,
        &context.org_id,
        ResourceKind::NotifyTemplate,
        CudAction::Updated,
        &saved.id.0,
        &saved,
    )
    .await;
    Ok(Json(saved))
}

#[permission("alerts.manage")]
async fn delete(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    let id = Id::from_string(id);
    if state
        .alerting
        .notify_engine
        .list_policies(&context.org_id)
        .await?
        .iter()
        .any(|policy| policy.template_id.as_ref() == Some(&id))
    {
        return Err(Error::conflict(
            "notify template is referenced by a notify policy",
        ));
    }
    state
        .alerting
        .templates
        .delete(&context.org_id, &id)
        .await?;
    emit_cud(
        &state,
        &context.org_id,
        ResourceKind::NotifyTemplate,
        CudAction::Deleted,
        &id.0,
        &delete_payload(&id.0),
    )
    .await;
    Ok(Json(serde_json::json!({"deleted": true})))
}
