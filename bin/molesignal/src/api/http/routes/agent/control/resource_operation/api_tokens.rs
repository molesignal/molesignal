// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde::Deserialize;
use serde_json::{Value, json};

use super::super::OperationOutcome;
use crate::{
    agent::model::ApprovalRequest,
    api::AppState,
    app::iam::IamContext,
    domain::iam::{
        access::IamPrincipalType,
        api_token::{ApiToken, ApiTokenKind},
    },
    infra::persistence::repositories::api_tokens::{
        assemble_token, generate_token_parts, hash_secret,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub(super) async fn execute(
    state: &AppState,
    ctx: &IamContext,
    approval: &ApprovalRequest,
) -> Result<OperationOutcome> {
    if ctx.is_system_scope() {
        return Err(Error::not_found("API tokens"));
    }
    match approval.action.as_str() {
        "create_api_token" => create(state, ctx, approval).await,
        "revoke_api_token" => revoke(state, ctx, approval).await,
        _ => unreachable!("API-token operation received unrelated action"),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateToken {
    name: String,
    #[serde(default)]
    role_id: Option<String>,
    #[serde(default)]
    expires_in_days: Option<i64>,
}

async fn create(
    state: &AppState,
    ctx: &IamContext,
    approval: &ApprovalRequest,
) -> Result<OperationOutcome> {
    if ctx.principal_type() != IamPrincipalType::User {
        return Err(Error::forbidden(
            "personal API tokens can only be created by an authenticated user",
        ));
    }
    let parameters: CreateToken = parse(&approval.parameters)?;
    let name = parameters.name.trim();
    if name.is_empty() || name.chars().count() > 255 {
        return Err(Error::invalid(
            "name must contain between 1 and 255 characters",
        ));
    }
    let role_id = match parameters.role_id {
        Some(role_id) => Id(role_id),
        None => {
            state
                .iam
                .service
                .iam_memberships
                .role_id_for_purpose(&ctx.org_id, "default_api_token")
                .await?
        }
    };
    let role = state.iam.roles.get(&ctx.org_id, &role_id).await?;
    if role.role_type != "organization" || role.scope != "organization" {
        return Err(Error::invalid(
            "API token role must have organization type and scope",
        ));
    }
    if role.key == "rum_client" {
        return Err(Error::invalid(
            "RUM client credentials must be issued through the application-bound token flow",
        ));
    }
    if role
        .permissions
        .iter()
        .any(|permission| !ctx.has_permission(permission))
    {
        return Err(Error::forbidden(
            "token role permissions cannot exceed executor IAM capabilities",
        ));
    }
    let (prefix, secret) = generate_token_parts();
    let plaintext = assemble_token(&prefix, &secret);
    let now = TimestampMicros::now();
    let expires_at = parameters.expires_in_days.map(|days| {
        let micros = days.clamp(1, 365 * 5).saturating_mul(86_400 * 1_000_000);
        TimestampMicros(now.0.saturating_add(micros))
    });
    let token = state
        .iam
        .api_tokens
        .create(ApiToken {
            id: Id(approval.target.clone()),
            prefix,
            secret_hash: hash_secret(&secret)?,
            org_id: ctx.org_id.clone(),
            user_id: approval.requested_by.clone(),
            role_id: role.id.clone(),
            name: name.to_string(),
            expires_at,
            last_used_at: None,
            revoked: false,
            created_at: now,
            is_default: false,
            token_kind: ApiTokenKind::Personal,
            application_id: None,
            service_account_id: None,
        })
        .await?;
    Ok(OperationOutcome::with_one_time_result(
        "personal API token created",
        json!({
            "verified": true,
            "api_token": {
                "id": token.id,
                "name": token.name,
                "prefix": token.prefix,
                "role_id": token.role_id,
                "role_key": role.key,
                "role_name": role.name,
                "token_kind": token.token_kind,
                "expires_at_micros": token.expires_at.map(|value| value.0),
                "created_at_micros": token.created_at.0
            }
        }),
        json!({
            "api_token": {
                "id": token.id,
                "name": token.name,
                "prefix": token.prefix,
                "token": plaintext,
                "role_id": token.role_id,
                "role_key": role.key,
                "role_name": role.name,
                "token_kind": token.token_kind,
                "expires_at_micros": token.expires_at.map(|value| value.0),
                "created_at_micros": token.created_at.0
            }
        }),
    ))
}

async fn revoke(
    state: &AppState,
    ctx: &IamContext,
    approval: &ApprovalRequest,
) -> Result<OperationOutcome> {
    let _: Empty = parse(&approval.parameters)?;
    let id = Id(approval.target.clone());
    let token = state.iam.api_tokens.get(&ctx.org_id, &id).await?;
    if !token.revoked {
        state.iam.api_tokens.mark_revoked(&ctx.org_id, &id).await?;
    }
    Ok(OperationOutcome {
        summary: "API token revoked".into(),
        verification: json!({"verified": true, "token_id": id, "revoked": true}),
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Empty {}

fn parse<T: serde::de::DeserializeOwned>(value: &Value) -> Result<T> {
    serde_json::from_value(value.clone())
        .map_err(|error| Error::invalid(format!("invalid operation parameters: {error}")))
}
