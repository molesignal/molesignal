// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::HashMap;

use serde::Deserialize;
use tool_runtime::{ToolResult, catalog::BuiltinToolKind};

use super::{
    ToolRuntime,
    common::{bounded, json_result, parse_args, redact_credentials},
};
use crate::{
    app::iam::IamContext,
    domain::iam::{IamAssignedRole, access::IamPrincipalType},
    infra::persistence::repositories::audit_events::AuditQuery,
    shared::{Error, Result, ids::Id},
};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct UserTargetArgs {
    target_user_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LimitArgs {
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AuditArgs {
    from_micros: Option<i64>,
    to_micros: Option<i64>,
    actor_kind: Option<String>,
    actor_id: Option<String>,
    action: Option<String>,
    target_kind: Option<String>,
    target_id: Option<String>,
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EmptyArgs {}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ServiceAccountArgs {
    service_account_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ApiTokenArgs {
    service_account_id: Option<String>,
    limit: Option<usize>,
}

pub(super) async fn execute(
    runtime: &ToolRuntime,
    auth: &IamContext,
    kind: BuiltinToolKind,
    arguments: serde_json::Value,
) -> Result<ToolResult> {
    match kind {
        BuiltinToolKind::GetUserProfile => {
            let args: UserTargetArgs = parse_args(arguments)?;
            let target_user_id = resolve_target_user(runtime, auth, args.target_user_id).await?;
            let user = runtime
                .administration
                .iam
                .users
                .get(&target_user_id)
                .await?;
            let org = runtime.administration.iam.orgs.get(&auth.org_id).await?;
            let (display_role, roles) = if target_user_id == auth.user_id {
                (auth.display_role.clone(), auth.roles.clone())
            } else {
                let roles = runtime
                    .administration
                    .iam
                    .iam_memberships
                    .assigned_roles(&target_user_id, &auth.org_id)
                    .await?;
                let display_role = roles
                    .iter()
                    .map(|role| role.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                (display_role, roles)
            };
            Ok(ToolResult::json(serde_json::json!({
                "user_id": user.id,
                "email": user.email,
                "display_name": user.display_name,
                "avatar_url": user.avatar_url,
                "bio": user.bio,
                "disabled": user.disabled,
                "status": user.status,
                "created_at": user.created_at,
                "org": org,
                "display_role": display_role,
                "roles": roles,
            })))
        }
        BuiltinToolKind::GetUserPreferences => {
            let args: UserTargetArgs = parse_args(arguments)?;
            let target_user_id = resolve_target_user(runtime, auth, args.target_user_id).await?;
            let preferences = runtime
                .administration
                .user_preferences
                .get(&target_user_id)
                .await?;
            Ok(ToolResult::json(serde_json::json!({
                "user_id": target_user_id,
                "theme": preferences.theme,
                "density": preferences.density,
                "language": preferences.language,
                "default_home_route": preferences.default_home_route,
                "time_format": preferences.time_format,
                "date_format": preferences.date_format,
                "timezone": preferences.timezone,
                "keyboard_shortcuts_enabled": preferences.keyboard_shortcuts_enabled,
            })))
        }
        BuiltinToolKind::ListOrganizationMembers => {
            let args: LimitArgs = parse_args(arguments)?;
            let memberships = bounded(
                runtime
                    .administration
                    .iam
                    .iam_memberships
                    .list_for_org(&auth.org_id)
                    .await?,
                args.limit,
                500,
            );
            let user_ids = memberships
                .iter()
                .map(|membership| membership.user_id.clone())
                .collect::<Vec<_>>();
            let mut users = runtime
                .administration
                .iam
                .users
                .get_many(&user_ids)
                .await?
                .into_iter()
                .map(|user| (user.id.0.clone(), user))
                .collect::<HashMap<_, _>>();
            let mut roles = runtime
                .administration
                .iam
                .iam_memberships
                .assigned_roles_for_users(&user_ids, &auth.org_id)
                .await?
                .into_iter()
                .map(|(user_id, roles)| (user_id.0, roles))
                .collect::<HashMap<_, _>>();
            let mut members = Vec::with_capacity(memberships.len());
            for membership in memberships {
                let user = users.remove(&membership.user_id.0).ok_or_else(|| {
                    Error::internal("organization membership references a missing user")
                })?;
                let assigned_roles = roles.remove(&membership.user_id.0).unwrap_or_default();
                members.push(serde_json::json!({
                    "user_id": user.id,
                    "email": user.email,
                    "display_name": user.display_name,
                    "avatar_url": user.avatar_url,
                    "bio": user.bio,
                    "disabled": user.disabled,
                    "status": user.status,
                    "joined_at": membership.joined_at,
                    "roles": assigned_roles,
                }));
            }
            Ok(ToolResult::json(serde_json::Value::Array(members)))
        }
        BuiltinToolKind::ListTeams => {
            let args: LimitArgs = parse_args(arguments)?;
            json_result(&bounded(
                runtime.administration.teams.list(&auth.org_id).await?,
                args.limit,
                500,
            ))
        }
        BuiltinToolKind::ListRoles => {
            let args: LimitArgs = parse_args(arguments)?;
            json_result(&bounded(
                runtime.administration.roles.list(&auth.org_id).await?,
                args.limit,
                500,
            ))
        }
        BuiltinToolKind::ListAuditEvents => {
            let args: AuditArgs = parse_args(arguments)?;
            let query = AuditQuery {
                from_micros: args.from_micros,
                to_micros: args.to_micros,
                actor_kind: args.actor_kind,
                actor_id: args.actor_id,
                action: args.action,
                target_kind: args.target_kind,
                target_id: args.target_id,
                limit: i64::try_from(args.limit.unwrap_or(50).clamp(1, 200)).unwrap_or(200),
                cursor: None,
            };
            let events = runtime
                .administration
                .audit_events
                .query(&auth.org_id, &query)
                .await?;
            let safe = serde_json::to_value(events)
                .map(|value| redact_credentials(&value))
                .map_err(|error| {
                    crate::shared::Error::internal(format!("serialize audit events: {error}"))
                })?;
            Ok(ToolResult::json(safe))
        }
        BuiltinToolKind::GetIamCapabilities => {
            let _: EmptyArgs = parse_args(arguments)?;
            Ok(ToolResult::json(serde_json::json!({
                "org_id": auth.org_id,
                "user_id": auth.user_id,
                "principal_type": auth.principal_type(),
                "principal_id": auth.principal_id(),
                "display_role": auth.display_role,
                "roles": auth.roles,
                "scope": auth.scope,
                "permissions": auth.permissions,
                "features": auth.features,
                "policy_version": auth.policy_version,
            })))
        }
        BuiltinToolKind::ListServiceAccounts => {
            let args: LimitArgs = parse_args(arguments)?;
            let accounts = bounded(
                runtime
                    .administration
                    .service_accounts
                    .list(&auth.org_id)
                    .await?,
                args.limit,
                500,
            );
            let role_ids = accounts
                .iter()
                .map(|account| account.role_id.clone())
                .collect::<Vec<_>>();
            let roles = runtime
                .administration
                .iam_access
                .repository()
                .role_summaries(&auth.org_id, &role_ids)
                .await?
                .into_iter()
                .map(|role| (role.id.0.clone(), role))
                .collect::<HashMap<_, _>>();
            let values = accounts
                .into_iter()
                .map(|account| {
                    let role = roles.get(&account.role_id.0).cloned().ok_or_else(|| {
                        Error::internal("service account references a missing IAM role")
                    })?;
                    Ok(service_account_json(account, role))
                })
                .collect::<Result<Vec<_>>>()?;
            Ok(ToolResult::json(
                serde_json::json!({"service_accounts": values}),
            ))
        }
        BuiltinToolKind::GetServiceAccount => {
            let args: ServiceAccountArgs = parse_args(arguments)?;
            let account = runtime
                .administration
                .service_accounts
                .get(&auth.org_id, &Id(args.service_account_id))
                .await?;
            Ok(ToolResult::json(
                resolve_service_account_json(runtime, auth, account).await?,
            ))
        }
        BuiltinToolKind::ListApiTokens => {
            let args: ApiTokenArgs = parse_args(arguments)?;
            let service_account_id = args
                .service_account_id
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(Id::from_string);
            let tokens = bounded(
                runtime
                    .administration
                    .api_tokens
                    .list_by_org(&auth.org_id)
                    .await?
                    .into_iter()
                    .filter(|token| {
                        service_account_id
                            .as_ref()
                            .is_none_or(|id| token.service_account_id.as_ref() == Some(id))
                    })
                    .collect(),
                args.limit,
                500,
            )
            .into_iter()
            .map(|token| {
                serde_json::json!({
                    "id": token.id,
                    "name": token.name,
                    "prefix": token.prefix,
                    "role_id": token.role_id,
                    "token_kind": token.token_kind,
                    "application_id": token.application_id,
                    "service_account_id": token.service_account_id,
                    "expires_at": token.expires_at,
                    "last_used_at": token.last_used_at,
                    "revoked": token.revoked,
                    "created_at": token.created_at,
                })
            })
            .collect::<Vec<_>>();
            Ok(ToolResult::json(serde_json::json!({"api_tokens": tokens})))
        }
        _ => unreachable!("administration executor received unrelated tool"),
    }
}

fn service_account_json(
    account: crate::domain::iam::service_account::ServiceAccount,
    role: IamAssignedRole,
) -> serde_json::Value {
    serde_json::json!({
        "id": account.id,
        "name": account.name,
        "description": account.description,
        "role": {"id": role.id, "key": role.key, "name": role.name},
        "disabled": account.disabled,
        "created_by": account.created_by,
        "created_at": account.created_at,
        "updated_at": account.updated_at,
    })
}

async fn resolve_service_account_json(
    runtime: &ToolRuntime,
    auth: &IamContext,
    account: crate::domain::iam::service_account::ServiceAccount,
) -> Result<serde_json::Value> {
    let role = runtime
        .administration
        .iam_access
        .repository()
        .role_summary(&auth.org_id, &account.role_id)
        .await?
        .ok_or_else(|| Error::internal("service account references a missing IAM role"))?;
    Ok(service_account_json(account, role))
}

async fn resolve_target_user(
    runtime: &ToolRuntime,
    auth: &IamContext,
    target_user_id: Option<String>,
) -> Result<Id> {
    let target_user_id = requested_user_id(auth, target_user_id)?;
    if target_user_id == auth.user_id {
        return Ok(target_user_id);
    }
    if !runtime
        .administration
        .iam_access
        .repository()
        .membership_exists(&auth.org_id, &target_user_id)
        .await?
    {
        return Err(Error::not_found("organization user"));
    }
    Ok(target_user_id)
}

fn requested_user_id(auth: &IamContext, target_user_id: Option<String>) -> Result<Id> {
    let Some(target_user_id) = target_user_id else {
        if auth.principal_type() == IamPrincipalType::ServiceAccount {
            return Err(Error::invalid(
                "target_user_id is required when the authenticated principal is a service account",
            ));
        }
        return Ok(auth.user_id.clone());
    };
    let target_user_id = target_user_id.trim();
    if target_user_id.is_empty() {
        return Err(Error::invalid("target_user_id must not be empty"));
    }
    let target_user_id = Id::from_string(target_user_id);
    if target_user_id != auth.user_id && !auth.has_permission("org.members.read") {
        return Err(Error::forbidden(
            "org.members.read permission is required for another user's data",
        ));
    }
    Ok(target_user_id)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::domain::iam::IamScope;

    fn auth(permissions: &[&str]) -> IamContext {
        IamContext {
            user_id: Id::from_string("user-a"),
            org_id: Id::from_string("org-a"),
            display_role: String::new(),
            roles: Vec::new(),
            credential_role_id: None,
            credential_application_id: None,
            credential_service_account_id: None,
            scope: IamScope::Organization,
            permissions: permissions
                .iter()
                .map(|value| (*value).to_string())
                .collect(),
            features: BTreeSet::new(),
            policy_version: 0,
        }
    }

    #[test]
    fn omitted_or_matching_target_resolves_to_the_authenticated_user() {
        let auth = auth(&[]);
        assert_eq!(requested_user_id(&auth, None).unwrap(), auth.user_id);
        assert_eq!(
            requested_user_id(&auth, Some(" user-a ".into())).unwrap(),
            auth.user_id
        );
        assert!(requested_user_id(&auth, Some("  ".into())).is_err());
    }

    #[test]
    fn another_user_requires_member_read_permission() {
        let denied = requested_user_id(&auth(&[]), Some("user-b".into()));
        assert!(denied.is_err());
        assert_eq!(
            requested_user_id(&auth(&["org.members.read"]), Some("user-b".into())).unwrap(),
            Id::from_string("user-b")
        );
    }

    #[test]
    fn service_account_is_not_treated_as_an_authenticated_user() {
        let mut auth = auth(&["org.members.read"]);
        auth.credential_service_account_id = Some(Id::from_string("service-account-a"));

        assert!(requested_user_id(&auth, None).is_err());
        assert_eq!(
            requested_user_id(&auth, Some("user-b".into())).unwrap(),
            Id::from_string("user-b")
        );
    }
}
