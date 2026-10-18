// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Agent Profile API：一种工作模式用哪个模型、哪些工具、什么数据范围，以及一轮对话的
//! 工具调用次数、总时限与温度。

use axum::{
    Extension, Json,
    extract::{Path, State},
};
use serde::Deserialize;
use serde_json::{Value, json};
use tool_runtime::ToolSurface;

use super::{default_true, validate_allowed_tools};
use crate::{
    agent::model::{AgentProfile, NetworkAccess},
    api::AppState,
    app::iam::IamContext,
    domain::iam::permission,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

#[derive(Debug, Deserialize)]
pub(super) struct AgentProfileRequest {
    name: String,
    #[serde(default)]
    description: String,
    model_provider_id: Option<Id>,
    model: Option<String>,
    #[serde(default)]
    allowed_tools: Vec<String>,
    #[serde(default)]
    data_scope: Value,
    #[serde(default)]
    risk_policy: Value,
    #[serde(default = "blocked_network")]
    network_access: NetworkAccess,
    #[serde(default = "default_context_tokens")]
    max_context_tokens: i32,
    #[serde(default = "default_investigation_secs")]
    max_investigation_secs: i32,
    #[serde(default = "default_tool_calls")]
    max_tool_calls: i32,
    /// 采样温度；不填时用 Mole Agent 的默认值。
    #[serde(default)]
    temperature: Option<f32>,
    /// 用它的对话能选 bypass permissions。
    #[serde(default)]
    allow_bypass_permissions: bool,
    #[serde(default)]
    is_default: bool,
    #[serde(default = "default_true")]
    enabled: bool,
}

fn blocked_network() -> NetworkAccess {
    NetworkAccess::Blocked
}

fn default_context_tokens() -> i32 {
    32_000
}

fn default_investigation_secs() -> i32 {
    1_800
}

fn default_tool_calls() -> i32 {
    32
}

fn validate_profile_request(request: &AgentProfileRequest) -> Result<()> {
    if request.max_tool_calls < 1 || request.max_tool_calls > 256 {
        return Err(Error::invalid("max_tool_calls must be between 1 and 256"));
    }

    if request.max_investigation_secs < 30 || request.max_investigation_secs > 7_200 {
        return Err(Error::invalid(
            "max_investigation_secs must be between 30 and 7200",
        ));
    }

    if request
        .temperature
        .is_some_and(|temperature| !(0.0..=2.0).contains(&temperature))
    {
        return Err(Error::invalid("temperature must be between 0 and 2"));
    }

    Ok(())
}

#[permission("agent.use")]
pub(super) async fn list_profiles(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Value>> {
    Ok(Json(json!({
        "profiles": state.agent.repository.list_profiles(&ctx.org_id).await?
    })))
}

#[permission("agent.manage")]
pub(super) async fn create_profile(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(request): Json<AgentProfileRequest>,
) -> Result<Json<AgentProfile>> {
    validate_profile_request(&request)?;

    validate_allowed_tools(
        &state,
        &ctx.org_id,
        &request.allowed_tools,
        ToolSurface::MoleAgent,
    )
    .await?;

    let now = TimestampMicros::now();

    let profile = AgentProfile {
        id: Id::new(),
        org_id: ctx.org_id.clone(),
        name: request.name,
        description: request.description,
        model_provider_id: request.model_provider_id,
        model: request.model,
        allowed_tools: request.allowed_tools,
        data_scope: request.data_scope,
        risk_policy: request.risk_policy,
        network_access: request.network_access,
        max_context_tokens: request.max_context_tokens,
        max_investigation_secs: request.max_investigation_secs,
        max_tool_calls: request.max_tool_calls,
        temperature: request.temperature,
        allow_bypass_permissions: request.allow_bypass_permissions,
        is_default: request.is_default,
        enabled: request.enabled,
        created_by: ctx.principal_id().clone(),
        created_at: now,
        updated_at: now,
    };

    Ok(Json(state.agent.repository.create_profile(profile).await?))
}

#[permission("agent.use")]
pub(super) async fn get_profile(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<AgentProfile>> {
    Ok(Json(
        state
            .agent
            .repository
            .get_profile(&ctx.org_id, &Id(id))
            .await?,
    ))
}

#[permission("agent.manage")]
pub(super) async fn update_profile(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(request): Json<AgentProfileRequest>,
) -> Result<Json<AgentProfile>> {
    validate_profile_request(&request)?;

    validate_allowed_tools(
        &state,
        &ctx.org_id,
        &request.allowed_tools,
        ToolSurface::MoleAgent,
    )
    .await?;

    let existing = state
        .agent
        .repository
        .get_profile(&ctx.org_id, &Id(id))
        .await?;

    let profile = AgentProfile {
        name: request.name,
        description: request.description,
        model_provider_id: request.model_provider_id,
        model: request.model,
        allowed_tools: request.allowed_tools,
        data_scope: request.data_scope,
        risk_policy: request.risk_policy,
        network_access: request.network_access,
        max_context_tokens: request.max_context_tokens,
        max_investigation_secs: request.max_investigation_secs,
        max_tool_calls: request.max_tool_calls,
        temperature: request.temperature,
        allow_bypass_permissions: request.allow_bypass_permissions,
        is_default: request.is_default,
        enabled: request.enabled,
        updated_at: TimestampMicros::now(),
        ..existing
    };

    Ok(Json(state.agent.repository.update_profile(profile).await?))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{AgentProfileRequest, validate_profile_request};
    use crate::agent::model::NetworkAccess;

    #[test]
    fn profile_network_access_accepts_blocked_and_allowed() {
        let defaulted: AgentProfileRequest =
            serde_json::from_value(json!({"name": "default"})).unwrap();
        assert_eq!(defaulted.network_access, NetworkAccess::Blocked);
        assert!(validate_profile_request(&defaulted).is_ok());

        let allowed: AgentProfileRequest =
            serde_json::from_value(json!({"name": "networked", "network_access": "allowed"}))
                .unwrap();
        assert_eq!(allowed.network_access, NetworkAccess::Allowed);
        assert!(validate_profile_request(&allowed).is_ok());

        assert!(
            serde_json::from_value::<AgentProfileRequest>(
                json!({"name": "invalid", "network_access": "unrestricted"}),
            )
            .is_err()
        );
    }

    #[test]
    fn temperature_and_run_time_stay_in_range() {
        let request = |body| serde_json::from_value::<AgentProfileRequest>(body).unwrap();

        let defaulted = request(json!({"name": "default"}));
        assert_eq!(defaulted.temperature, None);
        assert!(validate_profile_request(&defaulted).is_ok());

        assert!(
            validate_profile_request(&request(json!({"name": "a", "temperature": 0.0}))).is_ok()
        );
        assert!(
            validate_profile_request(&request(json!({"name": "a", "temperature": 1.5}))).is_ok()
        );
        assert!(
            validate_profile_request(&request(json!({"name": "a", "temperature": 2.5}))).is_err()
        );
        assert!(
            validate_profile_request(&request(json!({"name": "a", "temperature": -0.1}))).is_err()
        );
        assert!(
            validate_profile_request(&request(json!({"name": "a", "max_investigation_secs": 10})))
                .is_err()
        );
    }
}
