// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde::Deserialize;
use serde_json::{Value, json};

use super::super::OperationOutcome;
use crate::{
    agent::model::ApprovalRequest,
    api::{AppState, http::middleware::Permission},
    app::iam::IamContext,
    domain::function::{Function, FunctionLanguage},
    infra::persistence::repositories::functions::precheck_compile,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub(super) async fn execute(
    state: &AppState,
    ctx: &IamContext,
    approval: &ApprovalRequest,
) -> Result<OperationOutcome> {
    match approval.action.as_str() {
        "create_function" => create(state, ctx, approval).await,
        "update_function" => update(state, ctx, approval).await,
        "delete_function" => delete(state, ctx, approval).await,
        _ => unreachable!("function operation received unrelated action"),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FunctionWrite {
    name: String,
    language: FunctionLanguage,
    source: String,
    #[serde(default)]
    params_schema: Value,
}

async fn create(
    state: &AppState,
    ctx: &IamContext,
    approval: &ApprovalRequest,
) -> Result<OperationOutcome> {
    let parameters: FunctionWrite = parse(&approval.parameters)?;
    validate(state, &parameters)?;
    let now = TimestampMicros::now();
    let function = state
        .storage
        .functions
        .create(Function {
            id: Id(approval.target.clone()),
            org_id: ctx.org_id.clone(),
            name: parameters.name,
            language: parameters.language,
            source: parameters.source,
            params_schema: parameters.params_schema,
            created_at: now,
            updated_at: now,
        })
        .await?;
    outcome("function created", function)
}

async fn update(
    state: &AppState,
    ctx: &IamContext,
    approval: &ApprovalRequest,
) -> Result<OperationOutcome> {
    let parameters: FunctionWrite = parse(&approval.parameters)?;
    validate(state, &parameters)?;
    let existing = load_authorized(state, ctx, &approval.target, "functions.edit").await?;
    reject_builtin(&existing)?;
    let function = state
        .storage
        .functions
        .update(Function {
            id: existing.id,
            org_id: existing.org_id,
            name: parameters.name,
            language: parameters.language,
            source: parameters.source,
            params_schema: parameters.params_schema,
            created_at: existing.created_at,
            updated_at: TimestampMicros::now(),
        })
        .await?;
    outcome("function updated", function)
}

async fn delete(
    state: &AppState,
    ctx: &IamContext,
    approval: &ApprovalRequest,
) -> Result<OperationOutcome> {
    let _: Empty = parse(&approval.parameters)?;
    let function = load_authorized(state, ctx, &approval.target, "functions.delete").await?;
    reject_builtin(&function)?;
    state
        .storage
        .functions
        .delete(&function.org_id, &function.id)
        .await?;
    Ok(OperationOutcome {
        summary: "function deleted".into(),
        verification: json!({"verified": true, "function_id": function.id, "deleted": true}),
    })
}

async fn load_authorized(
    state: &AppState,
    ctx: &IamContext,
    id: &str,
    permission: &str,
) -> Result<Function> {
    let function = state
        .storage
        .functions
        .get_by_id(&Id(id.to_string()))
        .await?;
    if function.org_id != ctx.org_id && function.org_id.0 != "__builtin__" {
        return Err(Error::not_found("function not found"));
    }
    Permission::require_resource(
        state,
        ctx,
        permission,
        &ctx.org_id,
        "function",
        &function.id.0,
    )
    .await?;
    Ok(function)
}

fn validate(state: &AppState, value: &FunctionWrite) -> Result<()> {
    if value.name.trim().is_empty() {
        return Err(Error::invalid("name cannot be empty"));
    }
    precheck_compile(
        value.language,
        &value.source,
        state.storage.functions_js_runtime_enabled,
    )
}

fn reject_builtin(function: &Function) -> Result<()> {
    if function.org_id.0 == "__builtin__" {
        Err(Error::forbidden("built-in functions are read-only"))
    } else {
        Ok(())
    }
}

fn outcome(summary: &str, function: Function) -> Result<OperationOutcome> {
    Ok(OperationOutcome {
        summary: summary.into(),
        verification: json!({"verified": true, "function": function}),
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Empty {}

fn parse<T: serde::de::DeserializeOwned>(value: &Value) -> Result<T> {
    serde_json::from_value(value.clone())
        .map_err(|error| Error::invalid(format!("invalid operation parameters: {error}")))
}
