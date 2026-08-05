// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde::Deserialize;
use serde_json::{Value, json};

use super::super::control::{
    CreateApprovalRequest, create_agent_approval, dashboard_required_approvals,
};
use crate::{
    api::{AppState, http::middleware::Permission},
    app::iam::IamContext,
    intelligence::{
        tool_control::ToolExecutionMode,
        tools::{AgentExecutionPolicy, ToolAuthContext, ToolContent, ToolResult},
    },
    shared::{Error, Result, ids::Id},
};

pub(super) async fn get_capabilities(
    state: &AppState,
    auth: &IamContext,
    arguments: Value,
) -> Result<ToolResult> {
    require_authoring_access(auth)?;
    if arguments
        .as_object()
        .is_none_or(|values| !values.is_empty())
    {
        return Err(Error::invalid(
            "get_dashboard_capabilities does not accept arguments",
        ));
    }
    result_json(
        &state
            .intelligence
            .dashboard_authoring
            .capabilities()
            .await?,
    )
}

pub(super) async fn prepare(
    state: &AppState,
    auth: &IamContext,
    arguments: Value,
) -> Result<ToolResult> {
    require_authoring_access(auth)?;
    Permission::require_any_key(auth, &["streams.query", "sys.telemetry.read"])?;
    let prepared = state
        .intelligence
        .dashboard_authoring
        .prepare(auth.org_id.clone(), auth.user_id.clone(), arguments)
        .await?;
    result_json(&prepared)
}

pub(super) async fn propose(
    state: &AppState,
    auth: &IamContext,
    tool_auth: &ToolAuthContext,
    arguments: Value,
    execution_mode: ToolExecutionMode,
) -> Result<ToolResult> {
    require_authoring_access(auth)?;
    require_proposal_policy(tool_auth.execution_policy)?;
    let args: ProposeDashboardCreationArgs = serde_json::from_value(arguments)
        .map_err(|error| Error::invalid(format!("invalid tool arguments: {error}")))?;
    let draft_id = Id(args.draft_id);
    let draft = state
        .intelligence
        .dashboard_authoring
        .validate_reference(&auth.org_id, &auth.user_id, &draft_id, &args.expected_hash)
        .await?;
    let approval = create_agent_approval(
        state,
        auth,
        CreateApprovalRequest {
            investigation_id: tool_auth.investigation_id.clone().map(Id),
            action: "create_dashboard".into(),
            target: draft_id.0,
            parameters: json!({"expected_hash": args.expected_hash}),
            reason: args.reason,
            impact: args.impact,
            expires_at_micros: Some(draft.expires_at.0),
            required_approvals_override: Some(dashboard_required_approvals(execution_mode)?),
        },
    )
    .await?;
    Ok(ToolResult {
        content: vec![ToolContent::Json {
            json: json!({
                "approval": approval,
                "draft_id": draft.id,
                "model_hash": draft.model_hash,
                "message": "Dashboard 创建建议已提交；显式确认或审批并执行前不会创建 Dashboard。"
            }),
        }],
        is_error: false,
    })
}

fn require_authoring_access(auth: &IamContext) -> Result<()> {
    Permission::require_key(auth, "intelligence.use")?;
    Permission::require_key(auth, "dashboards.create")
}

fn require_proposal_policy(policy: AgentExecutionPolicy) -> Result<()> {
    if policy.allows_approval_request() {
        Ok(())
    } else {
        Err(Error::forbidden(
            "the current chat execution policy does not allow Dashboard creation proposals",
        ))
    }
}

fn result_json(value: &impl serde::Serialize) -> Result<ToolResult> {
    Ok(ToolResult {
        content: vec![ToolContent::Json {
            json: serde_json::to_value(value)
                .map_err(|error| Error::internal(error.to_string()))?,
        }],
        is_error: false,
    })
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProposeDashboardCreationArgs {
    draft_id: String,
    expected_hash: String,
    reason: String,
    impact: String,
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    fn auth_with(permissions: &[&str]) -> IamContext {
        IamContext {
            user_id: Id("user-1".into()),
            org_id: Id("org-1".into()),
            display_role: String::new(),
            roles: Vec::new(),
            credential_role_id: None,
            credential_application_id: None,
            scope: crate::domain::iam::IamScope::Organization,
            permissions: permissions
                .iter()
                .map(|value| (*value).to_string())
                .collect(),
            features: BTreeSet::new(),
            policy_version: 1,
        }
    }

    #[test]
    fn dashboard_authoring_requires_intelligence_and_dashboard_permissions() {
        assert!(require_authoring_access(&auth_with(&["intelligence.use"])).is_err());
        assert!(require_authoring_access(&auth_with(&["dashboards.create"])).is_err());
        assert!(
            require_authoring_access(&auth_with(&["intelligence.use", "dashboards.create"]))
                .is_ok()
        );
    }

    #[test]
    fn advice_and_read_only_policies_cannot_propose_dashboard_creation() {
        assert!(require_proposal_policy(AgentExecutionPolicy::AdviceOnly).is_err());
        assert!(require_proposal_policy(AgentExecutionPolicy::ReadOnly).is_err());
        assert!(require_proposal_policy(AgentExecutionPolicy::Policy).is_ok());
    }

    #[test]
    fn proposal_arguments_reject_compiled_models_and_identity_fields() {
        let valid = json!({
            "draft_id": "draft-1",
            "expected_hash": "sha256:abc",
            "reason": "Create an operational overview",
            "impact": "Adds one Dashboard"
        });
        assert!(serde_json::from_value::<ProposeDashboardCreationArgs>(valid.clone()).is_ok());

        for (field, value) in [
            ("org_id", json!("other-org")),
            ("user_id", json!("other-user")),
            ("folder_id", json!("other-folder")),
            ("compiled_model", json!({"schemaVersion": 2})),
        ] {
            let mut invalid = valid.clone();
            invalid.as_object_mut().unwrap().insert(field.into(), value);
            assert!(
                serde_json::from_value::<ProposeDashboardCreationArgs>(invalid).is_err(),
                "unexpectedly accepted `{field}`"
            );
        }
    }
}
