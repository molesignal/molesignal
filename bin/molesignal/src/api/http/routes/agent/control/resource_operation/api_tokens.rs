// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde::Deserialize;
use serde_json::{Value, json};

use super::super::OperationOutcome;
use crate::{
    agent::model::ApprovalRequest,
    api::{
        AppState,
        http::routes::api_tokens::{
            RequestedScope, TokenSpec, expires_at, issue_scope, new_token, validate_name,
        },
    },
    app::iam::IamContext,
    domain::iam::{IamScope, access::IamPrincipalType, api_token::ApiTokenKind},
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
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateToken {
    name: String,
    #[serde(default)]
    policy_id: Option<String>,
    #[serde(default)]
    permissions: Option<Vec<String>>,
    #[serde(default)]
    expires_in_days: Option<i64>,
}

/// A personal token for whoever asked for it. Like one issued on the account page, it comes from
/// a signed-in person, never from another token.
async fn create(
    state: &AppState,
    ctx: &IamContext,
    approval: &ApprovalRequest,
) -> Result<OperationOutcome> {
    if ctx.principal_type() != IamPrincipalType::User || ctx.scope != IamScope::Organization {
        return Err(Error::forbidden(
            "personal API tokens can only be created by a signed-in user",
        ));
    }

    let parameters: CreateToken = parse(&approval.parameters)?;
    let name = validate_name(&parameters.name)?;

    let scope = issue_scope(
        state,
        ctx,
        RequestedScope {
            policy_id: parameters.policy_id,
            permissions: parameters.permissions,
        },
    )
    .await?;

    let now = TimestampMicros::now();

    let (row, plaintext) = new_token(
        TokenSpec {
            id: Id(approval.target.clone()),
            org_id: ctx.org_id.clone(),
            user_id: approval.requested_by.clone(),
            name,
            scope,
            token_kind: ApiTokenKind::Personal,
            service_account_id: None,
            expires_at: expires_at(now, parameters.expires_in_days),
        },
        now,
    );

    let token = state.iam.api_tokens.create(row).await?;

    let metadata = json!({
        "id": token.id,
        "name": token.name,
        "prefix": token.prefix,
        "policyId": token.scope.policy_id(),
        "permissions": token.scope.permissions(),
        "tokenKind": token.token_kind,
        "expiresAt": token.expires_at.map(|value| value.0),
        "createdAt": token.created_at.0
    });

    let mut secret = metadata.clone();
    secret["token"] = Value::String(plaintext);

    Ok(OperationOutcome::with_one_time_result(
        "personal API token created",
        json!({"verified": true, "apiToken": metadata}),
        json!({"apiToken": secret}),
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
        verification: json!({"verified": true, "tokenId": id, "revoked": true}),
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Empty {}

fn parse<T: serde::de::DeserializeOwned>(value: &Value) -> Result<T> {
    serde_json::from_value(value.clone())
        .map_err(|error| Error::invalid(format!("invalid operation parameters: {error}")))
}
