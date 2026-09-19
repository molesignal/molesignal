// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::HashMap;

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    http::header::{CACHE_CONTROL, PRAGMA},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};

use crate::{
    api::{AppState, http::middleware::Permission},
    app::iam::IamContext,
    domain::iam::{
        IamAssignedRole,
        api_token::{ApiToken, ApiTokenKind},
        service_account::ServiceAccount,
    },
    infra::persistence::repositories::api_tokens::{
        assemble_token, generate_token_parts, hash_secret,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

const INITIAL_TOKEN_TTL_MICROS: i64 = 365 * 86_400 * 1_000_000;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/service-accounts", get(list).post(create))
        .route(
            "/service-accounts/{id}",
            get(get_one).patch(update).delete(delete),
        )
        .route("/service-accounts/{id}/enable", post(enable))
        .route("/service-accounts/{id}/disable", post(disable))
}

#[derive(Debug, Serialize)]
struct ServiceAccountResp {
    id: String,
    name: String,
    description: String,
    role_id: String,
    role_key: String,
    role_name: String,
    disabled: bool,
    created_by: String,
    created_at_micros: i64,
    updated_at_micros: i64,
}

#[derive(Debug, Serialize)]
struct ProvisionedServiceAccountResp {
    service_account: ServiceAccountResp,
    api_token: InitialApiTokenResp,
}

#[derive(Debug, Serialize)]
struct InitialApiTokenResp {
    id: String,
    name: String,
    prefix: String,
    /// Complete bearer token. Returned once and never stored in plaintext.
    token: String,
    role_id: String,
    role_key: String,
    role_name: String,
    token_kind: String,
    service_account_id: String,
    expires_at_micros: i64,
    created_at_micros: i64,
}

#[derive(Debug, Deserialize)]
struct CreateReq {
    name: String,
    #[serde(default)]
    description: String,
    role_id: String,
}

#[derive(Debug, Deserialize)]
struct UpdateReq {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    role_id: Option<String>,
}

fn account_resp(account: ServiceAccount, role: IamAssignedRole) -> ServiceAccountResp {
    ServiceAccountResp {
        id: account.id.0,
        name: account.name,
        description: account.description,
        role_id: role.id.0,
        role_key: role.key,
        role_name: role.name,
        disabled: account.disabled,
        created_by: account.created_by.0,
        created_at_micros: account.created_at.0,
        updated_at_micros: account.updated_at.0,
    }
}

async fn resolve_account_resp(
    state: &AppState,
    account: ServiceAccount,
) -> Result<ServiceAccountResp> {
    let role = state
        .iam
        .access
        .repository()
        .role_summary(&account.org_id, &account.role_id)
        .await?
        .ok_or_else(|| Error::internal("service account references a missing IAM role"))?;
    Ok(account_resp(account, role))
}

async fn list(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
) -> Result<Json<Vec<ServiceAccountResp>>> {
    require(&context, "service_accounts.read")?;
    let accounts = state.iam.service_accounts.list(&context.org_id).await?;
    let role_ids = accounts
        .iter()
        .map(|account| account.role_id.clone())
        .collect::<Vec<_>>();
    let roles = state
        .iam
        .access
        .repository()
        .role_summaries(&context.org_id, &role_ids)
        .await?
        .into_iter()
        .map(|role| (role.id.0.clone(), role))
        .collect::<HashMap<_, _>>();
    let responses = accounts
        .into_iter()
        .map(|account| {
            let role = roles
                .get(&account.role_id.0)
                .cloned()
                .ok_or_else(|| Error::internal("service account references a missing IAM role"))?;
            Ok(account_resp(account, role))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Json(responses))
}

async fn get_one(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<ServiceAccountResp>> {
    require(&context, "service_accounts.read")?;
    let account = state
        .iam
        .service_accounts
        .get(&context.org_id, &Id(id))
        .await?;
    Ok(Json(resolve_account_resp(&state, account).await?))
}

async fn create(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Json(request): Json<CreateReq>,
) -> Result<Response> {
    require(&context, "service_accounts.manage")?;
    let name = validate_name(&request.name)?;
    let description = validate_description(&request.description)?;
    let role_id = Id(request.role_id);
    validate_role(&state, &context, &role_id).await?;
    let now = TimestampMicros::now();
    let account_id = Id::new();
    let token_name = format!("{name} initial API token");
    let (prefix, secret) = generate_token_parts();
    let plaintext = assemble_token(&prefix, &secret);
    let initial_token = ApiToken {
        id: Id::new(),
        prefix,
        secret_hash: hash_secret(&secret)?,
        org_id: context.org_id.clone(),
        user_id: context.principal_id().clone(),
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
    let (saved_account, saved_token) = state
        .iam
        .service_accounts
        .provision(
            ServiceAccount {
                id: account_id,
                org_id: context.org_id.clone(),
                name,
                description,
                role_id,
                disabled: false,
                created_by: context.principal_id().clone(),
                created_at: now,
                updated_at: now,
            },
            initial_token,
        )
        .await?;
    let service_account = resolve_account_resp(&state, saved_account).await?;
    Ok(secret_response(ProvisionedServiceAccountResp {
        api_token: InitialApiTokenResp {
            id: saved_token.id.0,
            name: saved_token.name,
            prefix: saved_token.prefix,
            token: plaintext,
            role_id: service_account.role_id.clone(),
            role_key: service_account.role_key.clone(),
            role_name: service_account.role_name.clone(),
            token_kind: saved_token.token_kind.as_str().to_string(),
            service_account_id: service_account.id.clone(),
            expires_at_micros: saved_token
                .expires_at
                .expect("initial service-account API token always expires")
                .0,
            created_at_micros: saved_token.created_at.0,
        },
        service_account,
    }))
}

fn secret_response(payload: ProvisionedServiceAccountResp) -> Response {
    (
        [(CACHE_CONTROL, "private, no-store"), (PRAGMA, "no-cache")],
        Json(payload),
    )
        .into_response()
}

async fn update(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(id): Path<String>,
    Json(request): Json<UpdateReq>,
) -> Result<Json<ServiceAccountResp>> {
    require(&context, "service_accounts.manage")?;
    let mut account = state
        .iam
        .service_accounts
        .get(&context.org_id, &Id(id))
        .await?;
    if let Some(name) = request.name {
        account.name = validate_name(&name)?;
    }
    if let Some(description) = request.description {
        account.description = validate_description(&description)?;
    }
    if let Some(role_id) = request.role_id {
        let role_id = Id(role_id);
        validate_role(&state, &context, &role_id).await?;
        account.role_id = role_id;
    }
    account.updated_at = TimestampMicros::now();
    let saved = state.iam.service_accounts.update(account).await?;
    Ok(Json(resolve_account_resp(&state, saved).await?))
}

async fn enable(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<ServiceAccountResp>> {
    set_disabled(state, context, id, false).await
}

async fn disable(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<ServiceAccountResp>> {
    set_disabled(state, context, id, true).await
}

async fn set_disabled(
    state: AppState,
    context: IamContext,
    id: String,
    disabled: bool,
) -> Result<Json<ServiceAccountResp>> {
    require(&context, "service_accounts.manage")?;
    let account = state
        .iam
        .service_accounts
        .set_disabled(&context.org_id, &Id(id), disabled, TimestampMicros::now())
        .await?;
    Ok(Json(resolve_account_resp(&state, account).await?))
}

async fn delete(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    require(&context, "service_accounts.manage")?;
    state
        .iam
        .service_accounts
        .delete(&context.org_id, &Id(id), TimestampMicros::now())
        .await?;
    Ok(Json(serde_json::json!({"deleted": true})))
}

async fn validate_role(state: &AppState, context: &IamContext, role_id: &Id) -> Result<()> {
    let role = state.iam.roles.get(&context.org_id, role_id).await?;
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
        .any(|permission| !context.has_permission(permission))
    {
        return Err(Error::forbidden(
            "service account role permissions cannot exceed caller IAM capabilities",
        ));
    }
    Ok(())
}

pub(super) fn validate_name(value: &str) -> Result<String> {
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
    if value.chars().count() > 2000 {
        return Err(Error::invalid("description cannot exceed 2000 characters"));
    }
    Ok(value.to_string())
}

pub(super) fn require(context: &IamContext, permission: &str) -> Result<()> {
    if context.is_system_scope() {
        return Err(Error::not_found("service accounts"));
    }
    Permission::require_key(context, permission)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provision_response_with_plaintext_token_is_not_cacheable() {
        let response = secret_response(ProvisionedServiceAccountResp {
            service_account: ServiceAccountResp {
                id: "account-id".into(),
                name: "collector".into(),
                description: String::new(),
                role_id: "role-id".into(),
                role_key: "intake".into(),
                role_name: "Intake".into(),
                disabled: false,
                created_by: "user-id".into(),
                created_at_micros: 1,
                updated_at_micros: 1,
            },
            api_token: InitialApiTokenResp {
                id: "token-id".into(),
                name: "collector initial API token".into(),
                prefix: "0123456789abcdef".into(),
                token: "ms_0123456789abcdef_0123456789abcdef0123456789abcdef".into(),
                role_id: "role-id".into(),
                role_key: "intake".into(),
                role_name: "Intake".into(),
                token_kind: "service_account".into(),
                service_account_id: "account-id".into(),
                expires_at_micros: 2,
                created_at_micros: 1,
            },
        });
        assert_eq!(response.headers()[CACHE_CONTROL], "private, no-store");
        assert_eq!(response.headers()[PRAGMA], "no-cache");
    }
}
