// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Mole Agent 内置工具白名单解析。
//!
//! Schema 只接受 `{ "builtin": ["query_logs", ...] }`。动态 HTTP endpoint、
//! Shell、Browser 与任意 MCP 定义不会被解析，也没有执行路径。
//! 默认 Agent Profile 与组织 Toolset 均为收窄策略；两者同时存在时取交集。

use std::collections::{HashMap, HashSet};

use axum::Router;
use serde::Deserialize;

use crate::{
    api::AppState,
    intelligence::{
        model::{NetworkAccess, RiskLevel},
        tool_control::{McpServer, McpTool, ToolExecutionMode, ToolPolicy, ToolPolicyDefaults},
        tools::{is_builtin_tool, risk_for_tool},
    },
    shared::{Error, Result, ids::Id},
};

mod api;

pub fn routes() -> Router<AppState> {
    api::routes()
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ToolsetSchema {
    #[serde(default)]
    builtin: Option<Vec<String>>,
}

#[derive(Debug, Clone)]
pub struct ToolsetResolution {
    /// `None` 表示使用所有编译期注册工具；`Some` 表示进一步收窄。
    pub builtin_allowed: Option<HashSet<String>>,
    pub profile_allowed: Option<HashSet<String>>,
    pub builtin_policies: HashMap<String, ToolPolicy>,
    pub mcp_tools: HashMap<String, McpTool>,
    pub mcp_servers: HashMap<String, McpServer>,
    pub active_profile_id: Option<Id>,
    /// 来自默认 Agent Profile 的可信数据范围；多环境时取最严格的覆盖策略。
    pub active_environments: Vec<String>,
    pub network_access: NetworkAccess,
    pub default_risk_modes: serde_json::Value,
    pub default_environment_overrides: serde_json::Value,
}

impl ToolsetResolution {
    pub fn builtin_enabled(&self, name: &str) -> bool {
        is_builtin_tool(name)
            && self
                .builtin_allowed
                .as_ref()
                .is_none_or(|allowed| allowed.contains(name))
            && self.execution_mode_for_builtin(name) != ToolExecutionMode::Disabled
            && self
                .builtin_policies
                .get(name)
                .is_none_or(|policy| policy.enabled)
    }

    pub fn execution_mode_for_builtin(&self, name: &str) -> ToolExecutionMode {
        let risk = risk_for_tool(name).unwrap_or(RiskLevel::L4);
        let policy = self.builtin_policies.get(name);
        let base = policy
            .map(|policy| policy.execution_mode)
            .unwrap_or_else(|| self.default_mode_for_risk(risk));
        self.apply_environment_override(
            risk,
            base,
            policy.map(|policy| &policy.environment_overrides),
        )
    }

    pub fn execution_mode_for_mcp(&self, tool: &McpTool) -> ToolExecutionMode {
        let policy = self.builtin_policies.get(&tool.name);
        let base = policy
            .map(|policy| policy.execution_mode)
            .unwrap_or(tool.execution_mode);
        self.apply_environment_override(
            tool.risk,
            base,
            policy.map(|policy| &policy.environment_overrides),
        )
    }

    fn default_mode_for_risk(&self, risk: RiskLevel) -> ToolExecutionMode {
        let key = format!("{risk:?}").to_ascii_lowercase();
        self.default_risk_modes
            .get(&key)
            .and_then(serde_json::Value::as_str)
            .and_then(parse_execution_mode)
            .filter(|mode| mode.allowed_for_risk(risk))
            .unwrap_or_else(|| ToolExecutionMode::default_for_risk(risk))
    }

    fn apply_environment_override(
        &self,
        risk: RiskLevel,
        base: ToolExecutionMode,
        tool_overrides: Option<&serde_json::Value>,
    ) -> ToolExecutionMode {
        let risk_key = format!("{risk:?}").to_ascii_lowercase();
        self.active_environments
            .iter()
            .map(|environment| {
                tool_overrides
                    .and_then(|value| value.get(environment))
                    .and_then(serde_json::Value::as_str)
                    .or_else(|| {
                        self.default_environment_overrides
                            .get(environment)
                            .and_then(|value| value.get(&risk_key))
                            .and_then(serde_json::Value::as_str)
                    })
                    .and_then(parse_execution_mode)
                    .filter(|mode| mode.allowed_for_risk(risk))
                    .unwrap_or(base)
            })
            .reduce(stricter_mode)
            .unwrap_or(base)
    }

    pub fn tool_policy(&self, name: &str) -> Option<&ToolPolicy> {
        self.builtin_policies.get(name)
    }

    pub fn tool_enabled(&self, name: &str) -> bool {
        self.builtin_policies
            .get(name)
            .is_none_or(|policy| policy.enabled)
    }

    pub fn mcp_tool(&self, name: &str) -> Option<&McpTool> {
        self.mcp_tools.get(name).filter(|tool| {
            self.network_access == NetworkAccess::Allowed
                && tool.enabled
                && tool.status == crate::intelligence::tool_control::ManagedToolStatus::Healthy
                && self.tool_enabled(name)
                && self.execution_mode_for_mcp(tool) != ToolExecutionMode::Disabled
                && self
                    .profile_allowed
                    .as_ref()
                    .is_none_or(|allowed| allowed.contains(name))
                && self
                    .mcp_servers
                    .get(&tool.server_id.0)
                    .is_some_and(|server| server.enabled && server.status == "healthy")
        })
    }
}

impl Default for ToolsetResolution {
    fn default() -> Self {
        Self {
            builtin_allowed: None,
            profile_allowed: None,
            builtin_policies: HashMap::new(),
            mcp_tools: HashMap::new(),
            mcp_servers: HashMap::new(),
            active_profile_id: None,
            active_environments: Vec::new(),
            network_access: NetworkAccess::Blocked,
            default_risk_modes: serde_json::json!({
                "l0": "automatic",
                "l1": "confirmation",
                "l2": "single_approval",
                "l3": "dual_approval",
                "l4": "disabled"
            }),
            default_environment_overrides: serde_json::json!({}),
        }
    }
}

pub(crate) fn validate_toolset_schema(value: &serde_json::Value) -> Result<()> {
    let schema: ToolsetSchema = serde_json::from_value(value.clone())
        .map_err(|error| Error::invalid(format!("invalid toolset schema: {error}")))?;
    if let Some(names) = schema.builtin {
        for name in names {
            if !is_builtin_tool(&name) {
                return Err(Error::invalid(format!("tool `{name}` is not registered")));
            }
        }
    }
    Ok(())
}

pub async fn resolve_toolsets(state: &AppState, org_id: &Id) -> Result<ToolsetResolution> {
    resolve_toolsets_for_profile(state, org_id, None).await
}

pub async fn resolve_toolsets_for_profile(
    state: &AppState,
    org_id: &Id,
    requested_profile_id: Option<&Id>,
) -> Result<ToolsetResolution> {
    let toolsets = state.intelligence.toolsets.list(org_id).await?;
    let mut builtin_allowed: Option<HashSet<String>> = None;
    for toolset in toolsets.into_iter().filter(|toolset| toolset.enabled) {
        let parsed: ToolsetSchema = serde_json::from_value(toolset.schema).map_err(|error| {
            Error::invalid(format!(
                "invalid enabled Mole Agent toolset `{}`: {error}",
                toolset.name
            ))
        })?;
        if let Some(names) = parsed.builtin {
            let allowed = builtin_allowed.get_or_insert_with(HashSet::new);
            for name in names {
                if !is_builtin_tool(&name) {
                    return Err(Error::invalid(format!(
                        "tool `{name}` in enabled toolset `{}` is not registered",
                        toolset.name
                    )));
                }
                allowed.insert(name);
            }
        }
    }
    let profiles = state.intelligence.repository.list_profiles(org_id).await?;
    let active_profile = if let Some(profile_id) = requested_profile_id {
        Some(
            profiles
                .into_iter()
                .find(|profile| profile.enabled && profile.id == *profile_id)
                .ok_or_else(|| {
                    Error::invalid(format!(
                        "Agent Profile `{}` is not enabled or does not exist",
                        profile_id.0
                    ))
                })?,
        )
    } else {
        profiles
            .into_iter()
            .find(|profile| profile.enabled && profile.is_default)
    };
    let mcp_tools = state
        .intelligence
        .tool_control
        .list_mcp_tools(org_id, None)
        .await?;
    let registered_mcp: HashSet<String> = mcp_tools.iter().map(|tool| tool.name.clone()).collect();
    let profile_allowed = active_profile
        .as_ref()
        .map(|profile| -> Result<HashSet<String>> {
            let mut allowed = HashSet::new();
            for name in &profile.allowed_tools {
                if !is_builtin_tool(name) && !registered_mcp.contains(name) {
                    return Err(Error::invalid(format!(
                        "tool `{name}` in Agent Profile `{}` is not registered",
                        profile.name
                    )));
                }
                allowed.insert(name.clone());
            }
            Ok(allowed)
        })
        .transpose()?;
    let policies = state
        .intelligence
        .tool_control
        .list_policies(org_id)
        .await?
        .into_iter()
        .map(|policy| (policy.tool_name.clone(), policy))
        .collect();
    let defaults = state
        .intelligence
        .tool_control
        .get_policy_defaults(org_id)
        .await?
        .unwrap_or_else(|| {
            ToolPolicyDefaults::system_defaults(org_id.clone(), Id("system".into()))
        });
    let mcp_servers = state
        .intelligence
        .tool_control
        .list_mcp_servers(org_id)
        .await?
        .into_iter()
        .map(|server| (server.id.0.clone(), server))
        .collect();

    let builtin_profile_allowed = profile_allowed.as_ref().map(|allowed| {
        allowed
            .iter()
            .filter(|name| is_builtin_tool(name))
            .cloned()
            .collect()
    });
    let active_environments = active_profile
        .as_ref()
        .and_then(|profile| profile.data_scope.get("environments"))
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str)
        .filter(|environment| matches!(*environment, "development" | "staging" | "production"))
        .map(str::to_owned)
        .collect();

    Ok(ToolsetResolution {
        builtin_allowed: intersect_allowlists(builtin_profile_allowed, builtin_allowed),
        profile_allowed,
        builtin_policies: policies,
        mcp_tools: mcp_tools
            .into_iter()
            .map(|tool| (tool.name.clone(), tool))
            .collect(),
        mcp_servers,
        active_profile_id: active_profile.as_ref().map(|profile| profile.id.clone()),
        active_environments,
        network_access: active_profile
            .as_ref()
            .map(|profile| profile.network_access)
            .unwrap_or(NetworkAccess::Blocked),
        default_risk_modes: defaults.risk_modes,
        default_environment_overrides: defaults.environment_overrides,
    })
}

fn parse_execution_mode(value: &str) -> Option<ToolExecutionMode> {
    match value {
        "automatic" => Some(ToolExecutionMode::Automatic),
        "confirmation" => Some(ToolExecutionMode::Confirmation),
        "single_approval" => Some(ToolExecutionMode::SingleApproval),
        "dual_approval" => Some(ToolExecutionMode::DualApproval),
        "disabled" => Some(ToolExecutionMode::Disabled),
        _ => None,
    }
}

fn stricter_mode(left: ToolExecutionMode, right: ToolExecutionMode) -> ToolExecutionMode {
    if execution_mode_rank(right) > execution_mode_rank(left) {
        right
    } else {
        left
    }
}

const fn execution_mode_rank(mode: ToolExecutionMode) -> u8 {
    match mode {
        ToolExecutionMode::Automatic => 0,
        ToolExecutionMode::Confirmation => 1,
        ToolExecutionMode::SingleApproval => 2,
        ToolExecutionMode::DualApproval => 3,
        ToolExecutionMode::Disabled => 4,
    }
}

fn intersect_allowlists(
    profile: Option<HashSet<String>>,
    toolsets: Option<HashSet<String>>,
) -> Option<HashSet<String>> {
    match (profile, toolsets) {
        (None, None) => None,
        (Some(allowed), None) | (None, Some(allowed)) => Some(allowed),
        (Some(mut profile), Some(toolsets)) => {
            profile.retain(|name| toolsets.contains(name));
            Some(profile)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};

    use super::*;

    #[test]
    fn whitelist_can_only_narrow_registered_tools() {
        let allowed = HashSet::from(["query_logs".to_string()]);
        let resolution = ToolsetResolution {
            builtin_allowed: Some(allowed),
            ..ToolsetResolution::default()
        };
        assert!(resolution.builtin_enabled("query_logs"));
        assert!(!resolution.builtin_enabled("get_trace"));
        assert!(!resolution.builtin_enabled("shell"));
    }

    #[test]
    fn arbitrary_endpoint_schema_is_rejected() {
        let parsed = serde_json::from_value::<ToolsetSchema>(serde_json::json!({
            "builtin": ["query_logs"],
            "custom": [{
                "name": "arbitrary_http",
                "endpoint": {"url": "https://example.invalid"}
            }]
        }));
        assert!(parsed.is_err());
    }

    #[test]
    fn profile_and_toolset_allowlists_are_intersected() {
        let profile = HashSet::from(["query_logs".to_string(), "list_rum_sessions".to_string()]);
        let toolsets = HashSet::from(["query_logs".to_string(), "list_recent_alerts".to_string()]);
        assert_eq!(
            intersect_allowlists(Some(profile), Some(toolsets)),
            Some(HashSet::from(["query_logs".to_string()]))
        );
    }

    #[test]
    fn empty_profile_allowlist_disables_every_tool() {
        assert_eq!(
            intersect_allowlists(Some(HashSet::new()), None),
            Some(HashSet::new())
        );
    }

    #[test]
    fn active_environment_changes_the_effective_execution_mode() {
        let resolution = ToolsetResolution {
            active_environments: vec!["production".into()],
            default_environment_overrides: serde_json::json!({
                "production": {"l0": "confirmation"}
            }),
            ..ToolsetResolution::default()
        };
        assert_eq!(
            resolution.execution_mode_for_builtin("query_logs"),
            ToolExecutionMode::Confirmation
        );
    }

    #[test]
    fn multiple_active_environments_use_the_strictest_mode() {
        let resolution = ToolsetResolution {
            active_environments: vec!["development".into(), "production".into()],
            default_environment_overrides: serde_json::json!({
                "development": {"l0": "automatic"},
                "production": {"l0": "single_approval"}
            }),
            ..ToolsetResolution::default()
        };
        assert_eq!(
            resolution.execution_mode_for_builtin("query_logs"),
            ToolExecutionMode::SingleApproval
        );
    }

    #[test]
    fn development_override_can_relax_l1_within_the_hard_floor() {
        let resolution = ToolsetResolution {
            active_environments: vec!["development".into()],
            default_environment_overrides: serde_json::json!({
                "development": {"l1": "automatic"}
            }),
            ..ToolsetResolution::default()
        };
        assert_eq!(
            resolution.apply_environment_override(
                RiskLevel::L1,
                ToolExecutionMode::Confirmation,
                None
            ),
            ToolExecutionMode::Automatic
        );
    }

    #[test]
    fn profile_and_toolset_can_disable_dashboard_proposals() {
        let profile = HashSet::from([
            "prepare_dashboard".to_string(),
            "propose_dashboard_creation".to_string(),
        ]);
        let toolset = HashSet::from(["prepare_dashboard".to_string()]);
        let resolution = ToolsetResolution {
            builtin_allowed: intersect_allowlists(Some(profile), Some(toolset)),
            ..ToolsetResolution::default()
        };
        assert!(resolution.builtin_enabled("prepare_dashboard"));
        assert!(!resolution.builtin_enabled("propose_dashboard_creation"));
    }

    #[test]
    fn dashboard_policy_can_tighten_or_disable_proposals() {
        let now = crate::shared::time::TimestampMicros(1);
        let policy = |mode, enabled| ToolPolicy {
            org_id: Id("org-1".into()),
            tool_name: "propose_dashboard_creation".into(),
            enabled,
            execution_mode: mode,
            environment_overrides: serde_json::json!({}),
            timeout_ms: 30_000,
            max_calls_per_run: 1,
            max_response_bytes: 64_000,
            updated_by: Id("user-1".into()),
            created_at: now,
            updated_at: now,
        };
        for mode in [
            ToolExecutionMode::SingleApproval,
            ToolExecutionMode::DualApproval,
        ] {
            let resolution = ToolsetResolution {
                builtin_policies: HashMap::from([(
                    "propose_dashboard_creation".into(),
                    policy(mode, true),
                )]),
                ..ToolsetResolution::default()
            };
            assert!(resolution.builtin_enabled("propose_dashboard_creation"));
            assert_eq!(
                resolution.execution_mode_for_builtin("propose_dashboard_creation"),
                mode
            );
        }

        let disabled = ToolsetResolution {
            builtin_policies: HashMap::from([(
                "propose_dashboard_creation".into(),
                policy(ToolExecutionMode::Disabled, false),
            )]),
            ..ToolsetResolution::default()
        };
        assert!(!disabled.builtin_enabled("propose_dashboard_creation"));
    }
}
