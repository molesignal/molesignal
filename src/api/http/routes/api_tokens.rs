// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! API tokens HTTP（auth-hardening api-tokens）。
//!
//! - `POST /api/v1/auth/tokens` create（返一次性 plaintext `ms_<prefix>_<secret>`）
//! - `GET /api/v1/auth/tokens` list（无 secret_hash，无 plaintext）
//! - `DELETE /api/v1/auth/tokens/{id}` revoke

use axum::{
    Extension, Json, Router,
    extract::{Path, Query, State},
    http::header::{CACHE_CONTROL, PRAGMA},
    response::{IntoResponse, Response},
    routing::get,
};
use serde::{Deserialize, Serialize};

use crate::{
    api::{AppState, http::middleware::Permission},
    app::iam::IamContext,
    domain::{
        iam::api_token::{ApiToken, ApiTokenKind},
        rum::validate_application_id,
    },
    infra::persistence::repositories::api_tokens::{
        assemble_token, generate_token_parts, hash_secret,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/auth/tokens", get(list).post(create))
        .route("/auth/tokens/default", get(get_default))
        .route("/auth/tokens/rum", get(get_rum_client))
        .route("/auth/tokens/{id}", axum::routing::delete(revoke))
}

#[derive(Debug, Deserialize)]
pub struct CreateReq {
    pub name: String,
    /// 缺省使用数据库中 `default_api_token` purpose 对应的角色。
    #[serde(default)]
    pub role_id: Option<String>,
    /// 缺省 = 永不过期；建议 ≤ 365 天
    #[serde(default)]
    pub expires_in_days: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct CreateResp {
    pub id: String,
    pub prefix: String,
    /// 完整 `ms_*` token；**仅创建时返一次**
    pub token: String,
    pub role_id: String,
    pub role_key: String,
    pub role_name: String,
    pub token_kind: String,
    pub application_id: Option<String>,
    pub expires_at_micros: Option<i64>,
    pub created_at_micros: i64,
}

#[derive(Debug, Serialize)]
pub struct TokenResp {
    pub id: String,
    pub prefix: String,
    pub name: String,
    pub role_id: String,
    pub role_key: String,
    pub role_name: String,
    pub token_kind: String,
    pub application_id: Option<String>,
    pub expires_at_micros: Option<i64>,
    pub last_used_at_micros: Option<i64>,
    pub revoked: bool,
    pub created_at_micros: i64,
}

async fn to_resp(state: &AppState, t: ApiToken) -> Result<TokenResp> {
    let role = state
        .iam
        .access
        .repository()
        .role_summary(&t.org_id, &t.role_id)
        .await?
        .ok_or_else(|| Error::internal("API token references a missing IAM role"))?;
    Ok(TokenResp {
        id: t.id.0,
        prefix: t.prefix,
        name: t.name,
        role_id: role.id.0,
        role_key: role.key,
        role_name: role.name,
        token_kind: t.token_kind.as_str().to_string(),
        application_id: t.application_id,
        expires_at_micros: t.expires_at.map(|v| v.0),
        last_used_at_micros: t.last_used_at.map(|v| v.0),
        revoked: t.revoked,
        created_at_micros: t.created_at.0,
    })
}

async fn list(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Vec<TokenResp>>> {
    require_organization_scope(&ctx)?;
    Permission::require_key(&ctx, "api_tokens.read")?;
    let tokens = state.iam.api_tokens.list_by_org(&ctx.org_id).await?;
    let mut responses = Vec::with_capacity(tokens.len());
    for token in tokens {
        responses.push(to_resp(&state, token).await?);
    }
    Ok(Json(responses))
}

async fn create(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(req): Json<CreateReq>,
) -> Result<Response> {
    require_organization_scope(&ctx)?;
    Permission::require_key(&ctx, "api_tokens.manage")?;
    if req.name.trim().is_empty() {
        return Err(Error::invalid("name must not be empty"));
    }
    let role_id = match req.role_id {
        Some(role_id) => Id::from_string(role_id),
        None => {
            state
                .iam
                .service
                .iam_memberships
                .role_id_for_purpose(&ctx.org_id, "default_api_token")
                .await?
        }
    };
    let role = state
        .iam
        .access
        .repository()
        .role_summary(&ctx.org_id, &role_id)
        .await?
        .ok_or_else(|| Error::invalid("role_id must reference an IAM role in this organization"))?;
    if role.key == "rum_client" {
        return Err(Error::invalid(
            "use GET /auth/tokens/rum to issue an application-bound RUM client token",
        ));
    }
    let role_permissions = state
        .iam
        .access
        .repository()
        .role_permissions(&ctx.org_id, &role.id)
        .await?;
    if role_permissions
        .iter()
        .any(|permission| !ctx.has_permission(permission))
    {
        return Err(Error::forbidden(
            "token role permissions cannot exceed caller IAM capabilities",
        ));
    }
    let (prefix, secret) = generate_token_parts();
    let plaintext = assemble_token(&prefix, &secret);
    let secret_hash = hash_secret(&secret)?;
    let now = TimestampMicros::now();
    let expires_at = req.expires_in_days.map(|d| {
        let micros = d.clamp(1, 365 * 5).saturating_mul(86_400 * 1_000_000);
        TimestampMicros(now.0 + micros)
    });
    let row = ApiToken {
        id: Id::new(),
        prefix: prefix.clone(),
        secret_hash,
        org_id: ctx.org_id.clone(),
        user_id: ctx.user_id.clone(),
        role_id: role.id.clone(),
        name: req.name,
        expires_at,
        last_used_at: None,
        revoked: false,
        created_at: now,
        is_default: false,
        token_kind: ApiTokenKind::Personal,
        application_id: None,
    };
    let saved = state.iam.api_tokens.create(row).await?;
    Ok(secret_response(CreateResp {
        id: saved.id.0,
        prefix: saved.prefix,
        token: plaintext,
        role_id: role.id.0,
        role_key: role.key,
        role_name: role.name,
        token_kind: saved.token_kind.as_str().to_string(),
        application_id: saved.application_id,
        expires_at_micros: saved.expires_at.map(|v| v.0),
        created_at_micros: saved.created_at.0,
    }))
}

/// 取或建当前用户的默认接入 token，返完整明文（可重复回显）。非 RUM
/// 数据源接入页用它直接展示一个开箱即用的 ingestion token，免去手动创建。
async fn get_default(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Response> {
    require_organization_scope(&ctx)?;
    Permission::require_key(&ctx, "api_tokens.manage")?;
    let role_id = state
        .iam
        .service
        .iam_memberships
        .role_id_for_purpose(&ctx.org_id, "default_api_token")
        .await?;
    let role = state
        .iam
        .access
        .repository()
        .role_summary(&ctx.org_id, &role_id)
        .await?
        .ok_or_else(|| Error::internal("default API token IAM role is missing"))?;
    let default_permissions = state
        .iam
        .access
        .repository()
        .role_permissions(&ctx.org_id, &role.id)
        .await?;
    if default_permissions
        .iter()
        .any(|permission| !ctx.has_permission(permission))
    {
        return Err(Error::forbidden(
            "default token permissions cannot exceed caller IAM capabilities",
        ));
    }
    let dt = state
        .iam
        .api_tokens
        .ensure_default(&ctx.org_id, &ctx.user_id, &role.id)
        .await?;
    Ok(secret_response(CreateResp {
        id: dt.id.0,
        prefix: dt.prefix,
        token: dt.token,
        role_id: role.id.0,
        role_key: role.key,
        role_name: role.name,
        token_kind: dt.token_kind.as_str().to_string(),
        application_id: dt.application_id,
        expires_at_micros: None,
        created_at_micros: dt.created_at.0,
    }))
}

#[derive(Debug, Deserialize)]
pub struct RumClientParams {
    pub application_id: String,
}

/// Return the application-bound, write-only credential embedded by a RUM SDK.
async fn get_rum_client(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(params): Query<RumClientParams>,
) -> Result<Response> {
    require_organization_scope(&ctx)?;
    Permission::require_key(&ctx, "api_tokens.manage")?;
    let application_id = validate_application_id(&params.application_id)?;
    let role_id = state
        .iam
        .service
        .iam_memberships
        .role_id_for_purpose(&ctx.org_id, "rum_client_token")
        .await?;
    let role = state
        .iam
        .access
        .repository()
        .role_summary(&ctx.org_id, &role_id)
        .await?
        .ok_or_else(|| Error::internal("RUM client IAM role is missing"))?;
    let role_permissions = state
        .iam
        .access
        .repository()
        .role_permissions(&ctx.org_id, &role.id)
        .await?;
    if role_permissions
        .iter()
        .any(|permission| !ctx.has_permission(permission))
    {
        return Err(Error::forbidden(
            "RUM client token permissions cannot exceed caller IAM capabilities",
        ));
    }
    let token = state
        .iam
        .api_tokens
        .ensure_rum_client(&ctx.org_id, &ctx.user_id, &role.id, application_id)
        .await?;
    Ok(secret_response(CreateResp {
        id: token.id.0,
        prefix: token.prefix,
        token: token.token,
        role_id: role.id.0,
        role_key: role.key,
        role_name: role.name,
        token_kind: token.token_kind.as_str().to_string(),
        application_id: token.application_id,
        expires_at_micros: None,
        created_at_micros: token.created_at.0,
    }))
}

fn secret_response(payload: CreateResp) -> Response {
    (
        [(CACHE_CONTROL, "private, no-store"), (PRAGMA, "no-cache")],
        Json(payload),
    )
        .into_response()
}

async fn revoke(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    require_organization_scope(&ctx)?;
    Permission::require_key(&ctx, "api_tokens.manage")?;
    let id = Id(id);
    let _existing = state.iam.api_tokens.get(&ctx.org_id, &id).await?;
    state.iam.api_tokens.mark_revoked(&ctx.org_id, &id).await?;
    Ok(Json(serde_json::json!({ "revoked": true })))
}

fn require_organization_scope(context: &IamContext) -> Result<()> {
    if context.is_system_scope() {
        Err(Error::not_found("API tokens"))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::iam::IamScope;

    #[test]
    fn system_scope_cannot_manage_api_tokens_despite_its_display_role() {
        let context = IamContext {
            user_id: Id::from_string("root"),
            org_id: Id::from_string("_sys"),
            display_role: "Platform Steward".into(),
            roles: vec![crate::domain::iam::IamAssignedRole {
                id: Id::from_string("database-role"),
                key: "platform_steward".into(),
                name: "Platform Steward".into(),
                builtin: false,
            }],
            credential_role_id: None,
            credential_application_id: None,
            scope: IamScope::System,
            permissions: ["sys.licenses.read".to_string()].into_iter().collect(),
            features: std::collections::BTreeSet::new(),
            policy_version: 1,
        };

        assert!(matches!(
            require_organization_scope(&context),
            Err(Error::NotFound(_))
        ));
    }

    #[test]
    fn plaintext_token_responses_are_not_cacheable() {
        let response = secret_response(CreateResp {
            id: "token-id".into(),
            prefix: "0123456789abcdef".into(),
            token: "ms_0123456789abcdef_0123456789abcdef0123456789abcdef".into(),
            role_id: "role-id".into(),
            role_key: "ingest".into(),
            role_name: "Ingestion token".into(),
            token_kind: "default_ingestion".into(),
            application_id: None,
            expires_at_micros: None,
            created_at_micros: 1,
        });
        assert_eq!(response.headers()[CACHE_CONTROL], "private, no-store");
        assert_eq!(response.headers()[PRAGMA], "no-cache");
    }
}
