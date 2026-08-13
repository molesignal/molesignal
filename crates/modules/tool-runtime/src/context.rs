// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use async_trait::async_trait;
use domain::iam::{IamContext, access::IamPrincipalType};
use kernel::Result;
use serde::{Deserialize, Serialize};

use crate::{ToolCall, ToolResult, ToolSurface};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionPolicy {
    AdviceOnly,
    ReadOnly,
    #[default]
    Policy,
}

/// Trusted execution decision resolved from organization Tool Policy and environment overrides.
///
/// Adapters select this mode; tool arguments cannot relax it. The runtime uses the same value for
/// Mole Agent and inbound MCP calls so approval behavior cannot diverge by protocol.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolExecutionMode {
    Automatic,
    Confirmation,
    SingleApproval,
    DualApproval,
    #[default]
    Disabled,
}

impl ToolExecutionMode {
    pub const fn default_for_risk(risk: crate::RiskLevel) -> Self {
        match risk {
            crate::RiskLevel::L0 => Self::Automatic,
            crate::RiskLevel::L1 => Self::Confirmation,
            crate::RiskLevel::L2 => Self::SingleApproval,
            crate::RiskLevel::L3 => Self::DualApproval,
            crate::RiskLevel::L4 => Self::Disabled,
        }
    }

    /// System hard limit: policies may tighten a risk requirement, never weaken one.
    pub const fn allowed_for_risk(self, risk: crate::RiskLevel) -> bool {
        match risk {
            crate::RiskLevel::L0 | crate::RiskLevel::L1 => true,
            crate::RiskLevel::L2 | crate::RiskLevel::L3 => matches!(
                self,
                Self::SingleApproval | Self::DualApproval | Self::Disabled
            ),
            crate::RiskLevel::L4 => matches!(self, Self::DualApproval | Self::Disabled),
        }
    }

    pub const fn required_approvals(self) -> Option<i32> {
        match self {
            Self::Automatic | Self::Confirmation => Some(0),
            Self::SingleApproval => Some(1),
            Self::DualApproval => Some(2),
            Self::Disabled => None,
        }
    }

    pub const fn automatically_executes(self) -> bool {
        matches!(self, Self::Automatic)
    }
}

impl ExecutionPolicy {
    pub const fn allows_approval_request(self) -> bool {
        matches!(self, Self::Policy)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolCallSource {
    #[default]
    MoleAgent,
    InboundMcp,
    Automation,
    ManualTest,
}

impl ToolCallSource {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MoleAgent => "mole_agent",
            Self::InboundMcp => "inbound_mcp",
            Self::Automation => "automation",
            Self::ManualTest => "manual_test",
        }
    }

    /// Resolve the catalog surface whose exposure rules apply to this call source.
    pub const fn surface(self) -> ToolSurface {
        match self {
            Self::MoleAgent | Self::ManualTest => ToolSurface::MoleAgent,
            Self::InboundMcp => ToolSurface::InboundMcp,
            Self::Automation => ToolSurface::Automation,
        }
    }
}

/// Trusted tool invocation context derived from an authenticated [`IamContext`].
///
/// Identity fields are deliberately private and this type is not deserializable. Entry adapters
/// must start with [`ToolInvocationContext::from_iam`] rather than accepting organization or user
/// identity from tool arguments, request JSON, or unverified headers.
#[derive(Debug, Clone)]
pub struct ToolInvocationContext {
    iam: IamContext,
    chat_id: Option<String>,
    investigation_id: Option<String>,
    request_id: Option<String>,
    execution_policy: ExecutionPolicy,
    execution_mode: ToolExecutionMode,
    query_generation_only: bool,
    source: ToolCallSource,
}

impl ToolInvocationContext {
    /// Derive the immutable invocation identity from server-authenticated IAM state.
    pub fn from_iam(iam: &IamContext) -> Self {
        Self {
            iam: iam.clone(),
            chat_id: None,
            investigation_id: None,
            request_id: None,
            execution_policy: ExecutionPolicy::default(),
            execution_mode: ToolExecutionMode::default(),
            query_generation_only: false,
            source: ToolCallSource::default(),
        }
    }

    pub fn with_chat(mut self, chat_id: Option<String>) -> Self {
        self.chat_id = chat_id;
        self
    }

    pub fn with_investigation(mut self, investigation_id: Option<String>) -> Self {
        self.investigation_id = investigation_id;
        self
    }

    pub fn with_request(mut self, request_id: Option<String>) -> Self {
        self.request_id = request_id;
        self
    }

    pub fn with_source(mut self, source: ToolCallSource) -> Self {
        self.source = source;
        self
    }

    pub fn with_execution_policy(mut self, policy: ExecutionPolicy) -> Self {
        self.execution_policy = policy;
        self
    }

    pub fn with_execution_mode(mut self, mode: ToolExecutionMode) -> Self {
        self.execution_mode = mode;
        self
    }

    pub fn query_generation_only(mut self, enabled: bool) -> Self {
        self.query_generation_only = enabled;
        self
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

    /// Return the immutable server-authenticated IAM snapshot for authorization.
    pub fn iam_context(&self) -> &IamContext {
        &self.iam
    }

    pub fn chat_id(&self) -> Option<&str> {
        self.chat_id.as_deref()
    }

    pub fn investigation_id(&self) -> Option<&str> {
        self.investigation_id.as_deref()
    }

    pub fn request_id(&self) -> Option<&str> {
        self.request_id.as_deref()
    }

    pub const fn execution_policy(&self) -> ExecutionPolicy {
        self.execution_policy
    }

    pub const fn execution_mode(&self) -> ToolExecutionMode {
        self.execution_mode
    }

    pub const fn is_query_generation_only(&self) -> bool {
        self.query_generation_only
    }

    pub const fn source(&self) -> ToolCallSource {
        self.source
    }
}

#[async_trait]
pub trait ToolDispatcher: Send + Sync {
    async fn dispatch(&self, ctx: &ToolInvocationContext, call: ToolCall) -> Result<ToolResult>;
}

#[cfg(test)]
mod tests {
    use domain::{
        iam::{IamContext, IamScope, access::IamPrincipalType},
        shared::ids::Id,
    };

    use super::*;

    #[test]
    fn invocation_identity_is_derived_from_iam_and_survives_metadata_builders() {
        let mut iam = IamContext {
            user_id: Id("user-a".into()),
            org_id: Id("org-a".into()),
            display_role: String::new(),
            roles: Vec::new(),
            credential_role_id: Some(Id("role-a".into())),
            credential_application_id: None,
            credential_service_account_id: None,
            scope: IamScope::ApiToken,
            permissions: ["streams.query".to_string()].into_iter().collect(),
            features: Default::default(),
            policy_version: 0,
        };

        let context = ToolInvocationContext::from_iam(&iam)
            .with_source(ToolCallSource::InboundMcp)
            .with_chat(Some("chat-a".into()))
            .with_investigation(Some("investigation-a".into()))
            .with_request(Some("request-a".into()))
            .with_execution_policy(ExecutionPolicy::ReadOnly)
            .with_execution_mode(ToolExecutionMode::DualApproval)
            .query_generation_only(true);

        iam.user_id = Id("forged-user".into());
        iam.org_id = Id("forged-org".into());
        iam.permissions.clear();

        assert_eq!(context.user_id(), "user-a");
        assert_eq!(context.principal_id(), "user-a");
        assert_eq!(context.principal_type(), IamPrincipalType::User);
        assert_eq!(context.org_id(), "org-a");
        assert_eq!(context.iam_context().scope, IamScope::ApiToken);
        assert_eq!(
            context.iam_context().credential_role_id.as_ref(),
            Some(&Id("role-a".into()))
        );
        assert!(context.iam_context().has_permission("streams.query"));
        assert_eq!(context.chat_id(), Some("chat-a"));
        assert_eq!(context.investigation_id(), Some("investigation-a"));
        assert_eq!(context.request_id(), Some("request-a"));
        assert_eq!(context.execution_policy(), ExecutionPolicy::ReadOnly);
        assert_eq!(context.execution_mode(), ToolExecutionMode::DualApproval);
        assert!(context.is_query_generation_only());
        assert_eq!(context.source(), ToolCallSource::InboundMcp);
    }

    #[test]
    fn execution_mode_fails_closed_until_a_trusted_adapter_resolves_policy() {
        let iam = IamContext {
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

        assert_eq!(
            ToolInvocationContext::from_iam(&iam).execution_mode(),
            ToolExecutionMode::Disabled
        );
    }

    #[test]
    fn execution_modes_map_to_exact_review_counts() {
        assert_eq!(ToolExecutionMode::Automatic.required_approvals(), Some(0));
        assert_eq!(
            ToolExecutionMode::Confirmation.required_approvals(),
            Some(0)
        );
        assert_eq!(
            ToolExecutionMode::SingleApproval.required_approvals(),
            Some(1)
        );
        assert_eq!(
            ToolExecutionMode::DualApproval.required_approvals(),
            Some(2)
        );
        assert_eq!(ToolExecutionMode::Disabled.required_approvals(), None);
    }

    #[test]
    fn call_sources_map_to_enforced_catalog_surfaces() {
        assert_eq!(ToolCallSource::MoleAgent.surface(), ToolSurface::MoleAgent);
        assert_eq!(ToolCallSource::ManualTest.surface(), ToolSurface::MoleAgent);
        assert_eq!(
            ToolCallSource::InboundMcp.surface(),
            ToolSurface::InboundMcp
        );
        assert_eq!(
            ToolCallSource::Automation.surface(),
            ToolSurface::Automation
        );
    }

    #[test]
    fn service_account_principal_type_is_preserved() {
        let iam = IamContext {
            user_id: Id("collector-agent".into()),
            org_id: Id("org-a".into()),
            display_role: String::new(),
            roles: Vec::new(),
            credential_role_id: Some(Id("role-a".into())),
            credential_application_id: None,
            credential_service_account_id: Some(Id("collector-agent".into())),
            scope: IamScope::ApiToken,
            permissions: Default::default(),
            features: Default::default(),
            policy_version: 0,
        };

        let context = ToolInvocationContext::from_iam(&iam);
        assert_eq!(context.user_id(), "collector-agent");
        assert_eq!(context.principal_id(), "collector-agent");
        assert_eq!(context.principal_type(), IamPrincipalType::ServiceAccount);
    }
}
