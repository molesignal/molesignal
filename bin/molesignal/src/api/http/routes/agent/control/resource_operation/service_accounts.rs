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
        api_token::{ApiToken, ApiTokenKind},
        service_account::ServiceAccount,
    },
    infra::persistence::repositories::{
        api_tokens::{assemble_token, generate_token_parts, hash_secret},
        iam::roles::IamRole,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

const INITIAL_TOKEN_TTL_MICROS: i64 = 365 * 86_400 * 1_000_000;

pub(super) async fn execute(
    state: &AppState,
    ctx: &IamContext,
    approval: &ApprovalRequest,
) -> Result<OperationOutcome> {
    if ctx.is_system_scope() {
        return Err(Error::not_found("service accounts"));
    }
    match approval.action.as_str() {
        "create_service_account" => create(state, ctx, approval).await,
        "update_service_account" => update(state, ctx, approval).await,
        "enable_service_account" => set_disabled(state, ctx, approval, false).await,
        "disable_service_account" => set_disabled(state, ctx, approval, true).await,
        "delete_service_account" => delete(state, ctx, approval).await,
        _ => unreachable!("service-account operation received unrelated action"),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateServiceAccount {
    name: String,
    #[serde(default)]
    description: String,
    role_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateServiceAccount {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    role_id: Option<String>,
}

async fn create(
    state: &AppState,
    ctx: &IamContext,
    approval: &ApprovalRequest,
) -> Result<OperationOutcome> {
    let parameters: CreateServiceAccount = parse(&approval.parameters)?;
    let role_id = Id(parameters.role_id);
    let role = validate_role(state, ctx, &role_id).await?;
    let now = TimestampMicros::now();
    let account_id = Id(approval.target.clone());
    let name = validate_name(&parameters.name)?;
    let token_name = format!("{name} initial API token");
    let (prefix, secret) = generate_token_parts();
    let plaintext = assemble_token(&prefix, &secret);
    let initial_token = ApiToken {
        id: Id::new(),
        prefix,
        secret_hash: hash_secret(&secret)?,
        org_id: ctx.org_id.clone(),
        user_id: approval.requested_by.clone(),
        role_id: role_id.clone(),
        name: token_name,
        expires_at: Some(TimestampMicros(
            now.0.saturating_add(INITIAL_TOKEN_TTL_MICROS),
        )),
        last_used_at: None,
        revoked: false,
        created_at: now,
        is_default: false,
        token_kind: ApiTokenKind::ServiceAccount,
        application_id: None,
        service_account_id: Some(account_id.clone()),
    };
    let (account, token) = state
        .iam
        .service_accounts
        .provision(
            ServiceAccount {
                id: account_id,
                org_id: ctx.org_id.clone(),
                name,
                description: validate_description(&parameters.description)?,
                role_id,
                disabled: false,
                created_by: approval.requested_by.clone(),
                created_at: now,
                updated_at: now,
            },
            initial_token,
        )
        .await?;
    Ok(OperationOutcome::with_one_time_result(
        "service account and initial API token created",
        json!({
            "verified": true,
            "service_account": account,
            "api_token": {
                "id": token.id,
                "name": token.name,
                "prefix": token.prefix,
                "role_id": token.role_id,
                "role_key": role.key,
                "role_name": role.name,
                "token_kind": token.token_kind,
                "service_account_id": token.service_account_id,
                "expires_at_micros": token.expires_at.map(|value| value.0),
                "created_at_micros": token.created_at.0,
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
                "service_account_id": token.service_account_id,
                "expires_at_micros": token.expires_at.map(|value| value.0),
                "created_at_micros": token.created_at.0,
            }
        }),
    ))
}

async fn update(
    state: &AppState,
    ctx: &IamContext,
    approval: &ApprovalRequest,
) -> Result<OperationOutcome> {
    let parameters: UpdateServiceAccount = parse(&approval.parameters)?;
    if parameters.name.is_none() && parameters.description.is_none() && parameters.role_id.is_none()
    {
        return Err(Error::invalid(
            "update_service_account requires at least one field",
        ));
    }
    let mut account = state
        .iam
        .service_accounts
        .get(&ctx.org_id, &Id(approval.target.clone()))
        .await?;
    if let Some(name) = parameters.name {
        account.name = validate_name(&name)?;
    }
    if let Some(description) = parameters.description {
        account.description = validate_description(&description)?;
    }
    if let Some(role_id) = parameters.role_id {
        let role_id = Id(role_id);
        validate_role(state, ctx, &role_id).await?;
        account.role_id = role_id;
    }
    account.updated_at = TimestampMicros::now();
    let account = state.iam.service_accounts.update(account).await?;
    outcome("service account updated", account)
}

async fn set_disabled(
    state: &AppState,
    ctx: &IamContext,
    approval: &ApprovalRequest,
    disabled: bool,
) -> Result<OperationOutcome> {
    let _: EmptyParameters = parse(&approval.parameters)?;
    let account = state
        .iam
        .service_accounts
        .set_disabled(
            &ctx.org_id,
            &Id(approval.target.clone()),
            disabled,
            TimestampMicros::now(),
        )
        .await?;
    outcome(
        if disabled {
            "service account disabled and bound active API tokens revoked"
        } else {
            "service account enabled; revoked API tokens remain revoked"
        },
        account,
    )
}

async fn delete(
    state: &AppState,
    ctx: &IamContext,
    approval: &ApprovalRequest,
) -> Result<OperationOutcome> {
    let _: EmptyParameters = parse(&approval.parameters)?;
    let id = Id(approval.target.clone());
    state
        .iam
        .service_accounts
        .delete(&ctx.org_id, &id, TimestampMicros::now())
        .await?;
    Ok(OperationOutcome {
        summary: "service account deleted and bound active API tokens revoked".into(),
        verification: json!({"verified": true, "service_account_id": id, "deleted": true}),
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EmptyParameters {}

async fn validate_role(state: &AppState, ctx: &IamContext, role_id: &Id) -> Result<IamRole> {
    let role = state.iam.roles.get(&ctx.org_id, role_id).await?;
    if role.role_type != "organization" || role.scope != "organization" {
        return Err(Error::invalid(
            "service account role must have organization type and scope",
        ));
    }
    if role.key == "rum_client" {
        return Err(Error::invalid(
            "rum_client API tokens are application-scoped and cannot authenticate a service account",
        ));
    }
    if role
        .permissions
        .iter()
        .any(|permission| !ctx.has_permission(permission))
    {
        return Err(Error::forbidden(
            "service account role permissions cannot exceed executor IAM capabilities",
        ));
    }
    Ok(role)
}

fn validate_name(value: &str) -> Result<String> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > 128 {
        return Err(Error::invalid(
            "name must contain between 1 and 128 characters",
        ));
    }
    Ok(value.to_string())
}

fn validate_description(value: &str) -> Result<String> {
    let value = value.trim();
    if value.chars().count() > 2_000 {
        return Err(Error::invalid("description cannot exceed 2000 characters"));
    }
    Ok(value.to_string())
}

fn parse<T: serde::de::DeserializeOwned>(value: &Value) -> Result<T> {
    serde_json::from_value(value.clone())
        .map_err(|error| Error::invalid(format!("invalid operation parameters: {error}")))
}

fn outcome(summary: &str, account: ServiceAccount) -> Result<OperationOutcome> {
    Ok(OperationOutcome {
        summary: summary.into(),
        verification: json!({"verified": true, "service_account": account}),
    })
}
