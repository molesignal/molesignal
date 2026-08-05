// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Functions HTTP routes（spec functions-runtime）。

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    api::{AppState, http::middleware::ProtectedResource},
    app::{iam::IamContext, ingestion::FunctionExecutor},
    domain::{
        function::{Function, FunctionLanguage},
        iam::{permission, resource_permission},
    },
    infra::{persistence::repositories::functions::precheck_compile, runtime::VrlFunctionExecutor},
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/functions", get(list).post(create))
        .route("/functions/run", post(run))
        .route("/functions/{id}", get(get_one).put(update).delete(delete))
}

struct FunctionResource {
    value: Function,
    authorization_org_id: Id,
}

#[async_trait::async_trait]
impl ProtectedResource for FunctionResource {
    /// Built-in functions are global read-only resources. Authorize their use
    /// in the caller's organization while retaining the canonical persisted
    /// organization on `value`.
    type Id = (Id, Id);

    async fn load(state: &AppState, (id, caller_org_id): Self::Id) -> Result<Self> {
        let value = state.storage.functions.get_by_id(&id).await?;
        let authorization_org_id = if value.org_id.as_str() == "__builtin__" {
            caller_org_id
        } else {
            value.org_id.clone()
        };
        Ok(Self {
            value,
            authorization_org_id,
        })
    }

    fn organization_id(&self) -> &Id {
        &self.authorization_org_id
    }

    fn resource_type(&self) -> &str {
        "function"
    }

    fn resource_id(&self) -> &str {
        self.value.id.as_str()
    }
}

fn reject_builtin_write(function: &Function) -> Result<()> {
    if function.org_id.as_str() == "__builtin__" {
        Err(Error::forbidden("built-in functions are read-only"))
    } else {
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
pub struct CreateReq {
    pub name: String,
    pub language: FunctionLanguage,
    pub source: String,
    #[serde(default)]
    pub params_schema: Value,
}

#[derive(Debug, Deserialize)]
pub struct UpdateReq {
    pub name: String,
    pub language: FunctionLanguage,
    pub source: String,
    #[serde(default)]
    pub params_schema: Value,
}

#[derive(Debug, Deserialize)]
pub struct RunReq {
    pub language: FunctionLanguage,
    pub source: String,
    pub input: Value,
}

#[derive(Debug, Serialize)]
pub struct RunResp {
    pub output: Value,
}

#[derive(Debug, Serialize)]
pub struct Resp {
    pub id: String,
    pub name: String,
    pub language: FunctionLanguage,
    pub source: String,
    pub params_schema: Value,
    /// 内置预设（org `__builtin__`）：前端据此打「内置」标 + 只读，禁止编辑/删除。
    pub is_builtin: bool,
    /// 人类可读说明（内置预设从 params_schema.description 取；自建函数通常为空）。
    pub description: String,
    pub created_at_micros: i64,
    pub updated_at_micros: i64,
}

fn to_resp(f: Function) -> Resp {
    let is_builtin = f.org_id.0 == "__builtin__";
    let description = f
        .params_schema
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    Resp {
        id: f.id.0,
        name: f.name,
        language: f.language,
        source: f.source,
        params_schema: f.params_schema,
        is_builtin,
        description,
        created_at_micros: f.created_at.0,
        updated_at_micros: f.updated_at.0,
    }
}

#[permission("functions.read")]
async fn list(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Vec<Resp>>> {
    Ok(Json(
        state
            .storage
            .functions
            .list(&ctx.org_id)
            .await?
            .into_iter()
            .map(to_resp)
            .collect(),
    ))
}

#[resource_permission(
    action = "functions.read",
    resource = FunctionResource,
    id = (Id(id), ctx.org_id.clone()),
    bind = function
)]
async fn get_one(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Resp>> {
    Ok(Json(to_resp(function.value)))
}

#[permission("functions.create")]
async fn create(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(req): Json<CreateReq>,
) -> Result<Json<Resp>> {
    if req.name.is_empty() {
        return Err(Error::invalid("name cannot be empty"));
    }
    // POST 时同步编译校验（js-runtime-functions）
    precheck_compile(
        req.language,
        &req.source,
        state.storage.functions_js_runtime_enabled,
    )?;
    let now = TimestampMicros::now();
    let f = Function {
        id: Id::new(),
        org_id: ctx.org_id.clone(),
        name: req.name,
        language: req.language,
        source: req.source,
        params_schema: req.params_schema,
        created_at: now,
        updated_at: now,
    };
    let f = state.storage.functions.create(f).await?;
    Ok(Json(to_resp(f)))
}

#[resource_permission(
    action = "functions.edit",
    resource = FunctionResource,
    id = (Id(id), ctx.org_id.clone()),
    bind = function
)]
async fn update(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(req): Json<UpdateReq>,
) -> Result<Json<Resp>> {
    precheck_compile(
        req.language,
        &req.source,
        state.storage.functions_js_runtime_enabled,
    )?;
    let existing = function.value;
    reject_builtin_write(&existing)?;
    let f = Function {
        id: existing.id,
        org_id: existing.org_id,
        name: req.name,
        language: req.language,
        source: req.source,
        params_schema: req.params_schema,
        created_at: existing.created_at,
        updated_at: TimestampMicros::now(),
    };
    let f = state.storage.functions.update(f).await?;
    Ok(Json(to_resp(f)))
}

#[resource_permission(
    action = "functions.delete",
    resource = FunctionResource,
    id = (Id(id), ctx.org_id.clone()),
    bind = function
)]
async fn delete(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    reject_builtin_write(&function.value)?;
    state
        .storage
        .functions
        .delete(&function.value.org_id, &function.value.id)
        .await?;
    Ok(Json(serde_json::json!({"deleted": true})))
}

#[permission("functions.run")]
async fn run(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(req): Json<RunReq>,
) -> Result<Json<RunResp>> {
    precheck_compile(
        req.language,
        &req.source,
        state.storage.functions_js_runtime_enabled,
    )?;
    let now = TimestampMicros::now();
    let function = Function {
        id: Id::from_string("dry-run"),
        org_id: ctx.org_id.clone(),
        name: "dry-run".to_string(),
        language: req.language,
        source: req.source,
        params_schema: Value::Object(Default::default()),
        created_at: now,
        updated_at: now,
    };
    let mut output = req.input;
    let executor = VrlFunctionExecutor::new();
    executor.run(&function, &mut output).await?;
    Ok(Json(RunResp { output }))
}
