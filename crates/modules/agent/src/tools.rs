// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The Agent loop's side of the platform Tool Core.
//!
//! `tool-runtime` defines the tool catalog, risks, inputs, outputs, and execution policies.
//! This module holds the authentication context the server builds for an Agent run and the
//! dispatcher trait the server implements, so the model adapter layer does not depend on the
//! server.

use async_trait::async_trait;
use domain::iam::{IamContext, access::IamPrincipalType};
use kernel::Result;
use tool_runtime::{ExecutionPolicy, ToolCall, ToolResult};

/// Trusted Agent call context constructed by the server authentication layer.
///
/// `org_id` / `user_id` are never read from model-provided tool arguments.
#[derive(Debug, Clone)]
pub struct ToolAuthContext {
    iam: IamContext,
    chat_id: Option<String>,
    investigation_id: Option<String>,
    execution_policy: ExecutionPolicy,
    query_generation_only: bool,
}

impl ToolAuthContext {
    /// Construct Agent-loop context from the server-authenticated IAM context.
    pub fn from_iam(iam: &IamContext) -> Self {
        Self {
            iam: iam.clone(),
            chat_id: None,
            investigation_id: None,
            execution_policy: ExecutionPolicy::default(),
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

    pub const fn execution_policy(&self) -> ExecutionPolicy {
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

    pub fn with_execution_policy(mut self, policy: ExecutionPolicy) -> Self {
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

    /// Whether this call only reads and changes nothing, asks nobody and waits for nobody, so the
    /// loop may run it together with the calls next to it in the same model reply. The loop
    /// runs every call alone unless the dispatcher says so.
    fn runs_alongside(&self, call: &ToolCall) -> bool {
        let _ = call;
        false
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use domain::iam::{IamContext, IamScope};
    use kernel::ids::Id;
    use tool_runtime::{RiskLevel, ToolAccess, catalog::builtin_tools};

    use super::*;

    #[test]
    fn agent_tool_identity_is_an_immutable_iam_snapshot() {
        let mut iam = IamContext {
            user_id: Id("user-a".into()),
            org_id: Id("org-a".into()),
            display_role: String::new(),
            roles: Vec::new(),
            credential_token_id: None,
            credential_application_id: None,
            credential_service_account_id: None,
            scope: IamScope::Organization,
            permissions: Default::default(),
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
    fn only_the_modes_that_change_things_can_create_approval_requests() {
        assert!(!ExecutionPolicy::AdviceOnly.allows_approval_request());
        assert!(!ExecutionPolicy::ReadOnly.allows_approval_request());
        assert!(ExecutionPolicy::Policy.allows_approval_request());
        assert!(ExecutionPolicy::Auto.allows_approval_request());
        assert!(ExecutionPolicy::BypassPermissions.allows_approval_request());
    }
}
