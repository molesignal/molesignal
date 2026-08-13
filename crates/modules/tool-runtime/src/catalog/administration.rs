// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde_json::{Value, json};

use super::{BuiltinToolKind, object_schema, open_output};
use crate::{RiskLevel, ToolSpec};

pub(super) fn spec(kind: BuiltinToolKind) -> ToolSpec {
    let (description, category, input, permissions, tags) = match kind {
        BuiltinToolKind::GetUserProfile => (
            "Get safe profile fields for an organization user. User principals may omit target_user_id for themselves; service accounts must specify it, and other-user access requires org.members.read.",
            "identity",
            user_target_schema(),
            vec![],
            vec!["IAM", "Profile"],
        ),
        BuiltinToolKind::GetUserPreferences => (
            "Get workspace preferences for an organization user. User principals may omit target_user_id for themselves; service accounts must specify it, and other-user access requires org.members.read.",
            "identity",
            user_target_schema(),
            vec![],
            vec!["IAM", "Preferences"],
        ),
        BuiltinToolKind::ListOrganizationMembers => (
            "List members of the authenticated organization without password or credential data.",
            "directory",
            object_schema(json!({"limit": limit(500, 100)})),
            vec!["org.members.read"],
            vec!["IAM", "Users"],
        ),
        BuiltinToolKind::ListTeams => (
            "List teams in the authenticated organization.",
            "directory",
            object_schema(json!({"limit": limit(500, 100)})),
            vec!["org.members.read"],
            vec!["IAM", "Teams"],
        ),
        BuiltinToolKind::ListRoles => (
            "List roles and permission keys in the authenticated organization.",
            "authorization",
            object_schema(json!({"limit": limit(500, 100)})),
            vec!["iam.roles.read"],
            vec!["IAM", "Roles"],
        ),
        BuiltinToolKind::ListAuditEvents => (
            "Query bounded organization audit events with optional filters.",
            "audit",
            object_schema(json!({
                "from_micros": {"type": "integer"}, "to_micros": {"type": "integer"},
                "actor_kind": {"type": "string"}, "actor_id": {"type": "string"}, "action": {"type": "string"},
                "target_kind": {"type": "string"}, "target_id": {"type": "string"},
                "limit": limit(200, 50)
            })),
            vec!["audit.read"],
            vec!["Audit", "IAM"],
        ),
        BuiltinToolKind::GetIamCapabilities => (
            "Return the effective capability snapshot for the authenticated subject.",
            "authorization",
            object_schema(json!({})),
            vec![],
            vec!["IAM", "Capabilities"],
        ),
        BuiltinToolKind::ListServiceAccounts => (
            "List non-human principals in the authenticated organization.",
            "service_accounts",
            object_schema(json!({"limit": limit(500, 100)})),
            vec!["service_accounts.read"],
            vec!["IAM", "Service accounts"],
        ),
        BuiltinToolKind::GetServiceAccount => (
            "Get one non-human organization principal.",
            "service_accounts",
            id_input("service_account_id"),
            vec!["service_accounts.read"],
            vec!["IAM", "Service accounts"],
        ),
        BuiltinToolKind::ListApiTokens => (
            "List API token metadata in the authenticated organization without exposing token secrets; optionally filter by service account.",
            "api_tokens",
            object_schema(json!({
                "service_account_id": {
                    "type": "string",
                    "minLength": 1,
                    "description": "Optional Service Account principal bound to the API Token."
                },
                "limit": limit(500, 100)
            })),
            vec!["api_tokens.read"],
            vec!["IAM", "API tokens"],
        ),
        BuiltinToolKind::CreateApiToken => (
            "Create a personal API token for the requesting user. The plaintext token is returned once after approved execution and is never persisted in execution history.",
            "api_tokens",
            proposal_schema(
                vec!["name"],
                json!({
                    "name": {"type": "string", "minLength": 1, "maxLength": 255},
                    "role_id": {"type": "string", "minLength": 1},
                    "expires_in_days": {"type": "integer", "minimum": 1, "maximum": 1825}
                }),
            ),
            vec!["api_tokens.manage"],
            vec!["IAM", "API tokens", "CRUD", "Create"],
        ),
        BuiltinToolKind::RevokeApiToken => (
            "Revoke one API token.",
            "api_tokens",
            proposal_schema(
                vec!["token_id"],
                json!({"token_id": {"type": "string", "minLength": 1}}),
            ),
            vec!["api_tokens.manage"],
            vec!["IAM", "API tokens", "CRUD", "Revoke"],
        ),
        BuiltinToolKind::CreateServiceAccount => (
            "Atomically create one non-human organization principal and its initial bound API token. The plaintext token is returned once after execution and excluded from execution history.",
            "service_accounts",
            service_account_create_input(),
            vec!["service_accounts.manage"],
            vec!["IAM", "Service accounts", "Write"],
        ),
        BuiltinToolKind::UpdateServiceAccount => (
            "Update service-account metadata or its effective IAM role.",
            "service_accounts",
            service_account_update_input(),
            vec!["service_accounts.manage"],
            vec!["IAM", "Service accounts", "Write"],
        ),
        BuiltinToolKind::EnableServiceAccount => service_account_operation(
            "Enable one service account. Previously revoked API tokens stay revoked.",
            &["IAM", "Service accounts", "Enable"],
        ),
        BuiltinToolKind::DisableServiceAccount => service_account_operation(
            "Disable one service account and atomically revoke all bound active API tokens.",
            &["IAM", "Service accounts", "Disable"],
        ),
        BuiltinToolKind::DeleteServiceAccount => service_account_operation(
            "Delete one service account and atomically revoke all bound active API tokens.",
            &["IAM", "Service accounts", "Delete"],
        ),
        _ => unreachable!("administration catalog received unrelated kind"),
    };
    let tool = ToolSpec::read(
        kind.name(),
        description,
        "administration",
        category,
        input,
        open_output(),
        &permissions,
        &tags,
    );
    match kind {
        BuiltinToolKind::CreateServiceAccount => tool
            .managed_mutation(RiskLevel::L2, false, false)
            .mole_agent_only(),
        BuiltinToolKind::UpdateServiceAccount | BuiltinToolKind::EnableServiceAccount => {
            tool.managed_mutation(RiskLevel::L2, false, false)
        }
        BuiltinToolKind::DisableServiceAccount | BuiltinToolKind::DeleteServiceAccount => {
            tool.managed_mutation(RiskLevel::L3, true, true)
        }
        BuiltinToolKind::CreateApiToken => tool
            .managed_mutation(RiskLevel::L2, false, false)
            .mole_agent_only(),
        BuiltinToolKind::RevokeApiToken => tool.managed_mutation(RiskLevel::L3, true, true),
        _ => tool,
    }
}

fn user_target_schema() -> serde_json::Value {
    object_schema(json!({
        "target_user_id": {
            "type": "string",
            "minLength": 1,
            "description": "Stable ID of a user in the authenticated organization. User principals may omit it for themselves; Service Account principals must provide it. Reading another user requires org.members.read."
        }
    }))
}

fn limit(maximum: u32, default: u32) -> serde_json::Value {
    json!({"type": "integer", "minimum": 1, "maximum": maximum, "default": default})
}

fn id_input(field: &str) -> Value {
    json!({
        "type": "object", "required": [field],
        "properties": {(field): {"type": "string", "minLength": 1}},
        "additionalProperties": false
    })
}

fn proposal_schema(required: Vec<&str>, properties: Value) -> Value {
    let mut properties = properties.as_object().cloned().unwrap_or_default();
    properties.extend(
        json!({
            "reason": {"type": "string", "minLength": 1, "maxLength": 2000},
            "impact": {"type": "string", "minLength": 1, "maxLength": 2000},
            "expires_at_micros": {"type": "integer"}
        })
        .as_object()
        .cloned()
        .expect("static approval schema"),
    );
    json!({
        "type": "object",
        "required": required.into_iter().chain(["reason", "impact"]).collect::<Vec<_>>(),
        "properties": properties,
        "additionalProperties": false
    })
}

fn service_account_create_input() -> Value {
    proposal_schema(
        vec!["name", "role_id"],
        json!({
            "name": {"type": "string", "minLength": 1, "maxLength": 128},
            "description": {"type": "string", "maxLength": 2000},
            "role_id": {"type": "string", "minLength": 1}
        }),
    )
}

fn service_account_update_input() -> Value {
    proposal_schema(
        vec!["service_account_id"],
        json!({
            "service_account_id": {"type": "string", "minLength": 1},
            "name": {"type": "string", "minLength": 1, "maxLength": 128},
            "description": {"type": "string", "maxLength": 2000},
            "role_id": {"type": "string", "minLength": 1}
        }),
    )
}

fn service_account_operation(
    description: &'static str,
    tags: &[&'static str],
) -> (
    &'static str,
    &'static str,
    Value,
    Vec<&'static str>,
    Vec<&'static str>,
) {
    (
        description,
        "service_accounts",
        proposal_schema(
            vec!["service_account_id"],
            json!({"service_account_id": {"type": "string", "minLength": 1}}),
        ),
        vec!["service_accounts.manage"],
        tags.to_vec(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_tools_expose_an_optional_target_without_accepting_caller_identity() {
        for kind in [
            BuiltinToolKind::GetUserProfile,
            BuiltinToolKind::GetUserPreferences,
        ] {
            let spec = spec(kind);
            let properties = spec.input_schema["properties"]
                .as_object()
                .expect("user tool properties");
            assert!(properties.contains_key("target_user_id"));
            assert!(!properties.contains_key("user_id"));
            assert!(spec.input_schema.get("required").is_none());
            assert!(spec.description.contains("org.members.read"));
        }
    }

    #[test]
    fn credential_creation_stays_off_the_inbound_mcp_surface() {
        for kind in [
            BuiltinToolKind::CreateApiToken,
            BuiltinToolKind::CreateServiceAccount,
        ] {
            let spec = spec(kind);
            assert!(spec.exposure.mole_agent);
            assert!(!spec.exposure.inbound_mcp);
        }
    }
}
