// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::get,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    api::AppState,
    app::{
        iam::IamContext,
        notify::{
            ConnectorRegionsQuery, CreateNotifyConnector, UpdateNotifyConnector,
            mask_connector_config,
        },
    },
    domain::{
        iam::permission,
        notify::connector::{
            ConnectorAddress, ConnectorCapabilities, ConnectorRegion, ConnectorStatus,
            ConnectorTestStatus, NotifyConnector, NotifyMessage, NotifyTarget, NotifyTargetType,
        },
    },
    infra::connectors::aws,
    shared::{Result, ids::Id, time::TimestampMicros},
};

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/notify/connector-types", get(list_types))
        .route("/notify/aws-external-id", get(aws_external_id))
        .route(
            "/notify/connector-regions",
            axum::routing::post(connector_regions),
        )
        .route("/notify/connectors", get(list).post(create))
        .route(
            "/notify/connectors/{id}",
            get(get_one).put(update).delete(delete),
        )
        .route("/notify/connectors/{id}/test", axum::routing::post(test))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateConnectorRequest {
    name: String,
    #[serde(alias = "type")]
    connector_type: String,
    config: Value,
    #[serde(default = "default_true")]
    enabled: bool,
}

#[derive(Debug, Deserialize)]
struct UpdateConnectorRequest {
    name: String,
    #[serde(default)]
    config: Option<Value>,
    #[serde(default = "default_true")]
    enabled: bool,
}

/// 列区域：连接器类型与还在填的配置；编辑已有连接器时带它的 ID。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConnectorRegionsRequest {
    connector_type: String,
    #[serde(default)]
    config: Option<Value>,
    #[serde(default)]
    connector_id: Option<String>,
    #[serde(default)]
    language: Option<String>,
}

#[derive(Debug, Serialize)]
struct ConnectorRegionsResponse {
    regions: Vec<ConnectorRegion>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TestConnectorRequest {
    target_type: NotifyTargetType,
    target: String,
    #[serde(default)]
    message: Option<TestMessageRequest>,
    /// 套用这个通知模板发，占位符填示例值。
    #[serde(default)]
    template_id: Option<Id>,
}

#[derive(Debug, Deserialize)]
struct TestMessageRequest {
    #[serde(default)]
    title: String,
    #[serde(default)]
    text: String,
    #[serde(default)]
    markdown: Option<String>,
    #[serde(default)]
    html: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ConnectorResponse {
    id: Id,
    organization_id: Id,
    name: String,
    connector_type: String,
    config: Value,
    capabilities: ConnectorCapabilities,
    enabled: bool,
    status: ConnectorStatus,
    last_tested_at: Option<TimestampMicros>,
    last_test_status: Option<ConnectorTestStatus>,
    last_test_error: Option<String>,
    created_at: TimestampMicros,
    updated_at: TimestampMicros,
}

/// 一种连接器：能力，以及配置里服务商地址的目录，连接器里照它显示、选择地址。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ConnectorTypeResponse {
    connector_type: &'static str,
    capabilities: ConnectorCapabilities,
    addresses: Vec<ConnectorAddress>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ConnectorTestResponse {
    sent: bool,
    tested_at: i64,
    elapsed_ms: u64,
    provider_message_id: Option<String>,
    error: Option<String>,
}

fn default_true() -> bool {
    true
}

fn to_response(connector: NotifyConnector) -> ConnectorResponse {
    ConnectorResponse {
        id: connector.id,
        organization_id: connector.organization_id,
        name: connector.name,
        connector_type: connector.connector_type,
        config: mask_connector_config(&connector.config),
        capabilities: connector.capabilities,
        enabled: connector.enabled,
        status: connector.status,
        last_tested_at: connector.last_tested_at,
        last_test_status: connector.last_test_status,
        last_test_error: connector.last_test_error,
        created_at: connector.created_at,
        updated_at: connector.updated_at,
    }
}

fn test_message(request: Option<TestMessageRequest>) -> NotifyMessage {
    let request = request.unwrap_or(TestMessageRequest {
        title: String::new(),
        text: String::new(),
        markdown: None,
        html: None,
    });

    NotifyMessage {
        title: if request.title.trim().is_empty() {
            "MoleSignal notify connector test".into()
        } else {
            request.title
        },
        text: if request.text.trim().is_empty() {
            "Your MoleSignal notify connector is configured correctly.".into()
        } else {
            request.text
        },
        markdown: request.markdown,
        html: request.html,
        metadata: Default::default(),
        provider_templates: Default::default(),
        event: None,
        card: None,
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AwsExternalIdResponse {
    external_id: String,
}

/// 本组织的 AWS 连接器扮演角色时带的外部 ID，供写进角色的信任策略。
#[permission("notify.connectors.read")]
async fn aws_external_id(
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<AwsExternalIdResponse>> {
    Ok(Json(AwsExternalIdResponse {
        external_id: aws::external_id(&ctx.org_id),
    }))
}

#[permission("notify.connectors.read")]
async fn list_types(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Vec<ConnectorTypeResponse>>> {
    Ok(Json(
        state
            .alerting
            .notify
            .supported_connector_types()
            .into_iter()
            .map(|supported| ConnectorTypeResponse {
                connector_type: supported.connector_type,
                capabilities: supported.capabilities,
                addresses: supported.addresses,
            })
            .collect(),
    ))
}

/// 分区域的连接器（阿里云、AWS）能用的区域，向厂商查。要凭证的用请求里的配置，所以与新建、修改
/// 连接器要一样的权限。
#[permission("notify.connectors.manage")]
async fn connector_regions(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(request): Json<ConnectorRegionsRequest>,
) -> Result<Json<ConnectorRegionsResponse>> {
    let regions = state
        .alerting
        .notify
        .connector_regions(
            &ctx.org_id,
            ConnectorRegionsQuery {
                connector_type: request.connector_type,
                config: request.config,
                connector_id: request.connector_id.map(Id::from_string),
                language: request.language.unwrap_or_else(|| "en-us".into()),
            },
        )
        .await?;

    Ok(Json(ConnectorRegionsResponse { regions }))
}

#[permission("notify.connectors.read")]
async fn list(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Vec<ConnectorResponse>>> {
    Ok(Json(
        state
            .alerting
            .notify
            .list_connectors(&ctx.org_id)
            .await?
            .into_iter()
            .map(to_response)
            .collect(),
    ))
}

#[permission("notify.connectors.read")]
async fn get_one(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<ConnectorResponse>> {
    Ok(Json(to_response(
        state
            .alerting
            .notify
            .get_connector(&ctx.org_id, &Id::from_string(id))
            .await?,
    )))
}

#[permission("notify.connectors.manage")]
async fn create(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(request): Json<CreateConnectorRequest>,
) -> Result<(StatusCode, Json<ConnectorResponse>)> {
    let connector = state
        .alerting
        .notify
        .create_connector(
            &ctx.org_id,
            CreateNotifyConnector {
                name: request.name,
                connector_type: request.connector_type,
                config: request.config,
                enabled: request.enabled,
            },
        )
        .await?;

    Ok((StatusCode::CREATED, Json(to_response(connector))))
}

#[permission("notify.connectors.manage")]
async fn update(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(request): Json<UpdateConnectorRequest>,
) -> Result<Json<ConnectorResponse>> {
    let connector = state
        .alerting
        .notify
        .update_connector(
            &ctx.org_id,
            &Id::from_string(id),
            UpdateNotifyConnector {
                name: request.name,
                config: request.config,
                enabled: request.enabled,
            },
        )
        .await?;

    Ok(Json(to_response(connector)))
}

#[permission("notify.connectors.delete")]
async fn delete(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<StatusCode> {
    state
        .alerting
        .notify
        .delete_connector(&ctx.org_id, &Id::from_string(id))
        .await?;

    Ok(StatusCode::NO_CONTENT)
}

#[permission("notify.connectors.manage")]
async fn test(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(request): Json<TestConnectorRequest>,
) -> Result<Json<ConnectorTestResponse>> {
    let outcome = state
        .alerting
        .notify
        .test_connector(
            &ctx.org_id,
            &Id::from_string(id),
            NotifyTarget {
                target_type: request.target_type,
                value: request.target,
                metadata: Default::default(),
            },
            test_message(request.message),
            request.template_id.as_ref(),
        )
        .await?;

    Ok(Json(ConnectorTestResponse {
        sent: outcome.sent,
        tested_at: outcome.tested_at.0,
        elapsed_ms: outcome.elapsed_ms,
        provider_message_id: outcome.provider_message_id,
        error: outcome.error,
    }))
}
