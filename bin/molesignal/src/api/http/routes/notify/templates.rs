// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Notify 模板 CRUD。

use std::collections::{BTreeMap, BTreeSet};

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    routing::get,
};
use serde::{Deserialize, Serialize};

use crate::{
    api::{
        AppState,
        http::{
            federation::{delete_payload, emit_cud},
            interface_language::interface_language_pack,
        },
    },
    app::{iam::IamContext, notify::validate_notify_template},
    domain::{
        federation::{CudAction, ResourceKind},
        iam::permission,
        notify::{
            connector::NotifyChannel,
            policy::NotifyPolicy,
            preference::NotifyCategory,
            template::{
                NOTIFY_TEMPLATE_PRESET_LANGUAGE_PREFIX, NotifyCard, NotifyTemplateField,
                NotifyTemplatePreset, ProviderTemplate, notify_template_field_catalog,
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
#[serde(rename_all = "camelCase")]
struct WriteRequest {
    name: String,
    body: String,
    #[serde(default)]
    format: Option<String>,
    #[serde(default = "default_template_category")]
    category: NotifyCategory,
    /// 空着是通用模板。
    #[serde(default)]
    channel: Option<NotifyChannel>,
    #[serde(default)]
    provider_templates: BTreeMap<String, ProviderTemplate>,
    /// 即时通讯的 App 与群机器人发成的卡片；只有即时通讯模板有。
    #[serde(default)]
    card: Option<NotifyCard>,
}

const fn default_template_category() -> NotifyCategory {
    NotifyCategory::Alert
}

fn validate_request(request: &WriteRequest) -> Result<String> {
    if request.name.trim().is_empty() {
        return Err(Error::invalid("name must not be empty"));
    }

    let format = request.format.as_deref().unwrap_or("text");

    validate_notify_template(
        request.channel,
        format,
        &request.body,
        &request.provider_templates,
        request.card.as_ref(),
    )?;

    Ok(format.to_string())
}

/// 用着模板 `id` 的一个通知策略：当通用模板，或给哪个渠道另选的模板。
fn used_by<'a>(policies: &'a [NotifyPolicy], id: &Id) -> Option<&'a NotifyPolicy> {
    policies.iter().find(|policy| {
        policy.template_id.as_ref() == Some(id)
            || policy
                .channel_templates
                .values()
                .any(|template_id| template_id == id)
    })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TemplateFieldsResponse {
    fields: Vec<NotifyTemplateField>,
    presets: Vec<NotifyTemplatePreset>,
    label_keys: Vec<String>,
    annotation_keys: Vec<String>,
}

/// 模板的变量与预置模板；预置模板的正文是登录的人读界面用的语言。
#[permission("notify.templates.read")]
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

    let preset_bodies =
        interface_language_pack(&state, &context, NOTIFY_TEMPLATE_PRESET_LANGUAGE_PREFIX).await;

    Ok(Json(TemplateFieldsResponse {
        fields: notify_template_field_catalog(),
        presets: notify_template_preset_catalog(&preset_bodies),
        label_keys: label_keys.into_iter().collect(),
        annotation_keys: annotation_keys.into_iter().collect(),
    }))
}

#[permission("notify.templates.read")]
async fn list(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
) -> Result<Json<Vec<NotifyTemplateRecord>>> {
    Ok(Json(state.alerting.templates.list(&context.org_id).await?))
}

#[permission("notify.templates.manage")]
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
            channel: request.channel,
            provider_templates: request.provider_templates,
            card: request.card,
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

#[permission("notify.templates.read")]
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

#[permission("notify.templates.manage")]
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

    // 策略按渠道选模板：被用着的模板换了渠道，那个策略的那个渠道就用错了模板。
    if request.channel != existing.channel {
        let policies = state
            .alerting
            .notify_engine
            .list_policies(&context.org_id)
            .await?;

        if let Some(policy) = used_by(&policies, &existing.id) {
            return Err(Error::conflict(format!(
                "notify template is used by notify policy {}: choose another template there before changing its channel",
                policy.name
            )));
        }
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
            channel: request.channel,
            provider_templates: request.provider_templates,
            card: request.card,
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

#[permission("notify.templates.manage")]
async fn delete(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    let id = Id::from_string(id);

    let policies = state
        .alerting
        .notify_engine
        .list_policies(&context.org_id)
        .await?;

    if used_by(&policies, &id).is_some() {
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
