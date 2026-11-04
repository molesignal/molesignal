// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::HashMap;

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    response::Response,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};

use crate::{
    api::{
        AppState,
        http::{
            middleware::Permission,
            routes::api_tokens::{IssuedTokenResp, TokenSpec, new_token, secret_response},
        },
    },
    app::iam::IamContext,
    domain::iam::{
        IamAssignedRole,
        api_token::{ApiTokenKind, ApiTokenScope},
        service_account::ServiceAccount,
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
#[serde(rename_all = "camelCase")]
struct ServiceAccountResp {
    id: String,
    name: String,
    description: String,
    role_id: String,
    role_key: String,
    role_name: String,
    disabled: bool,
    created_by: String,
    created_at: i64,
    updated_at: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProvisionedServiceAccountResp {
    service_account: ServiceAccountResp,
    /// The account's first token, which may do whatever the account may.
    api_token: IssuedTokenResp,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateReq {
    name: String,
    #[serde(default)]
    description: String,
    role_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
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
        created_at: account.created_at.0,
        updated_at: account.updated_at.0,
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

    let (initial_token, plaintext) = new_token(
        TokenSpec {
            id: Id::new(),
            org_id: context.org_id.clone(),
            user_id: context.principal_id().clone(),
            name: format!("{name} initial API token"),
            scope: ApiTokenScope::Principal,
            token_kind: ApiTokenKind::ServiceAccount,
            service_account_id: Some(account_id.clone()),
            expires_at: Some(TimestampMicros(
                now.0.saturating_add(INITIAL_TOKEN_TTL_MICROS),
            )),
        },
        now,
    );

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
        api_token: IssuedTokenResp {
            meta: saved_token.into(),
            token: plaintext,
        },
        service_account,
    }))
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
    require(&context, "service_accounts.delete")?;

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

    super::grant_limit::require_held(context, &role.effective_permissions)?;

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
    use axum::http::header::{CACHE_CONTROL, PRAGMA};

    use super::*;
    use crate::api::http::routes::api_tokens::{ScopeResp, TokenResp};

    #[test]
    fn provision_response_with_plaintext_token_is_not_cacheable() {
        let response = secret_response(ProvisionedServiceAccountResp {
            service_account: ServiceAccountResp {
                id: "account-id".into(),
                name: "collector".into(),
                description: String::new(),
                role_id: "role-id".into(),
                role_key: "editor".into(),
                role_name: "Editor".into(),
                disabled: false,
                created_by: "user-id".into(),
                created_at: 1,
                updated_at: 1,
            },
            api_token: IssuedTokenResp {
                meta: TokenResp {
                    id: "token-id".into(),
                    prefix: "0123456789abcdef".into(),
                    name: "collector initial API token".into(),
                    token_kind: "service_account".into(),
                    user_id: "user-id".into(),
                    scope: ScopeResp {
                        policy_id: None,
                        permissions: None,
                    },
                    application_id: None,
                    service_account_id: Some("account-id".into()),
                    expires_at: Some(2),
                    last_used_at: None,
                    revoked: false,
                    created_at: 1,
                },
                token: "mspat-0123456789abcdefghjkmnpqrstvwxyz0123".into(),
            },
        });
        assert_eq!(response.headers()[CACHE_CONTROL], "private, no-store");
        assert_eq!(response.headers()[PRAGMA], "no-cache");
    }
}
