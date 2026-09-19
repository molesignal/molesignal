// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Mole Agent 对平台 Tool Core 的兼容适配。
//!
//! 工具 catalog、风险、输入输出与执行策略由 `tool-runtime` 统一定义。这里仅保留
//! Agent loop 现有的认证上下文和 dispatcher trait，避免模型适配层反向依赖服务端。

use async_trait::async_trait;
use domain::iam::{IamContext, access::IamPrincipalType};
pub use tool_runtime::{
    ExecutionPolicy as AgentExecutionPolicy, PermissionMode, ToolAccess, ToolAnnotations, ToolCall,
    ToolContent, ToolExecutionMode, ToolExposure, ToolResult, ToolSpec as Tool, ToolSurface,
    catalog::{
        BuiltinToolKind, builtin_tools, is_builtin_tool, is_meta_tool, pinned_tools, risk_for_tool,
        tools_for_surface,
    },
};

use crate::shared::Result;

/// 由服务端认证层构造的可信 Agent 调用上下文。
///
/// `org_id` / `user_id` 永远不从模型提供的 tool arguments 读取。
#[derive(Debug, Clone)]
pub struct ToolAuthContext {
    iam: IamContext,
    chat_id: Option<String>,
    investigation_id: Option<String>,
    execution_policy: AgentExecutionPolicy,
    query_generation_only: bool,
}

impl ToolAuthContext {
    /// Construct Agent-loop context from the server-authenticated IAM context.
    pub fn from_iam(iam: &IamContext) -> Self {
        Self {
            iam: iam.clone(),
            chat_id: None,
            investigation_id: None,
            execution_policy: AgentExecutionPolicy::default(),
            query_generation_only: false,
        }
    }

    pub fn user_id(&self) -> &str {
        &self.iam.user_id.0
    }

    pub fn principal_id(&self) -> &str {
        &self.iam.principal_id().0
    }

    pub fn principal_type(&self) -> IamPrincipalType {
        self.iam.principal_type()
    }

    pub fn org_id(&self) -> &str {
        &self.iam.org_id.0
    }

    /// Return the immutable server-authenticated IAM snapshot carried by this Agent run.
    pub fn iam_context(&self) -> &IamContext {
        &self.iam
    }

    pub fn chat_id(&self) -> Option<&str> {
        self.chat_id.as_deref()
    }

    pub fn investigation_id(&self) -> Option<&str> {
        self.investigation_id.as_deref()
    }

    pub const fn execution_policy(&self) -> AgentExecutionPolicy {
        self.execution_policy
    }

    pub const fn is_query_generation_only(&self) -> bool {
        self.query_generation_only
    }

    pub fn with_chat(mut self, chat_id: Option<String>) -> Self {
        self.chat_id = chat_id;
        self
    }

    pub fn with_investigation(mut self, investigation_id: Option<String>) -> Self {
        self.investigation_id = investigation_id;
        self
    }

    pub fn with_execution_policy(mut self, policy: AgentExecutionPolicy) -> Self {
        self.execution_policy = policy;
        self
    }

    pub fn query_generation_only(mut self, enabled: bool) -> Self {
        self.query_generation_only = enabled;
        self
    }
}

#[async_trait]
pub trait ToolDispatcher: Send + Sync {
    async fn dispatch(&self, ctx: &ToolAuthContext, call: ToolCall) -> Result<ToolResult>;
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use domain::{
        iam::{IamContext, IamScope},
        shared::ids::Id,
    };

    use super::*;
    use crate::agent::model::RiskLevel;

    #[test]
    fn agent_tool_identity_is_an_immutable_iam_snapshot() {
        let mut iam = IamContext {
            user_id: Id("user-a".into()),
            org_id: Id("org-a".into()),
            display_role: String::new(),
            roles: Vec::new(),
            credential_role_id: None,
            credential_application_id: None,
            credential_service_account_id: None,
            scope: IamScope::Organization,
            permissions: Default::default(),
            features: Default::default(),
            policy_version: 0,
        };
        let context = ToolAuthContext::from_iam(&iam).with_chat(Some("chat-a".into()));

        iam.user_id = Id("forged-user".into());
        iam.org_id = Id("forged-org".into());

        assert_eq!(context.user_id(), "user-a");
        assert_eq!(context.principal_id(), "user-a");
        assert_eq!(context.principal_type(), IamPrincipalType::User);
        assert_eq!(context.org_id(), "org-a");
        assert_eq!(context.chat_id(), Some("chat-a"));
        assert_eq!(context.iam_context().scope, IamScope::Organization);
    }

    #[test]
    fn registry_contains_only_explicit_tools() {
        let names = builtin_tools()
            .into_iter()
            .map(|tool| tool.name)
            .collect::<HashSet<_>>();
        assert!(names.contains("query_logs"));
        assert!(names.contains("tool_search"));
        assert!(names.contains("get_incident"));
        assert!(!names.contains("shell"));
        assert!(!names.contains("http"));
        assert!(!names.contains("browser"));
    }

    #[test]
    fn proposal_tools_cannot_execute_directly() {
        let tool = builtin_tools()
            .into_iter()
            .find(|tool| tool.name == "propose_operation")
            .expect("proposal tool");
        assert_eq!(tool.access, ToolAccess::CreatesApprovalRequest);
        assert_eq!(tool.risk, RiskLevel::L3);
    }

    #[test]
    fn only_policy_mode_can_create_approval_requests() {
        assert!(!AgentExecutionPolicy::AdviceOnly.allows_approval_request());
        assert!(!AgentExecutionPolicy::ReadOnly.allows_approval_request());
        assert!(AgentExecutionPolicy::Policy.allows_approval_request());
    }
}
