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
    api::{
        AppState,
        http::{interface_language::interface_language_pack, middleware::ProtectedResource},
    },
    app::iam::IamContext,
    domain::{
        function::{Function, FunctionLanguage, FunctionOutcome},
        i18n_language::LanguagePack,
        iam::{permission, resource_permission},
    },
    infra::persistence::repositories::functions::precheck_compile,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

/// What each built-in function does is in the `i18n_language` table under
/// this prefix: `function.preset.<name>.description`, the name with `_` for
/// `-`. The key says preset, the baseline's own word for these functions: a
/// seed row that opens on a text holding "builtin" is read as a resource id
/// that encodes what it is, and refused by the baseline's tests.
const BUILTIN_FUNCTION_KEY: &str = "function.preset";

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
    /// What the function reads beside the event (VRL `%name`), such as the stream a pipeline read
    /// the event from.
    #[serde(default)]
    pub metadata: serde_json::Map<String, Value>,
}

#[derive(Debug, Serialize)]
pub struct RunResp {
    pub output: Value,
    /// The function dropped the event on purpose (VRL `abort`): it would go no further.
    pub dropped: bool,
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
    /// 人类可读说明。内置预设按读者的界面语言取自 `i18n_language` 表，取不到时用
    /// params_schema.description；自建函数只有后者，通常为空。
    pub description: String,
    pub created_at_micros: i64,
    pub updated_at_micros: i64,
}

fn to_resp(f: Function) -> Resp {
    described(f, &LanguagePack::default())
}

/// The function as the API returns it. A built-in one is described in the
/// reader's language when `descriptions` has its entry.
fn described(f: Function, descriptions: &LanguagePack) -> Resp {
    let is_builtin = f.org_id.0 == "__builtin__";

    let in_reader_language = if is_builtin {
        descriptions.get(&builtin_description_key(&f.name))
    } else {
        None
    };

    let description = in_reader_language
        .or_else(|| f.params_schema.get("description").and_then(Value::as_str))
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
    let functions = state.storage.functions.list(&ctx.org_id).await?;
    let descriptions = interface_language_pack(&state, &ctx, BUILTIN_FUNCTION_KEY).await;

    Ok(Json(
        functions
            .into_iter()
            .map(|function| described(function, &descriptions))
            .collect(),
    ))
}

/// A language key holds no `-`, and a built-in function's name does.
fn builtin_description_key(name: &str) -> String {
    format!(
        "{BUILTIN_FUNCTION_KEY}.{}.description",
        name.replace('-', "_")
    )
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
    if function.value.org_id.as_str() != "__builtin__" {
        return Ok(Json(to_resp(function.value)));
    }

    let descriptions = interface_language_pack(&state, &ctx, BUILTIN_FUNCTION_KEY).await;
    Ok(Json(described(function.value, &descriptions)))
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

    let outcome = state
        .storage
        .function_executor
        .run_with_metadata(&function, &mut output, &req.metadata)
        .await?;

    Ok(Json(RunResp {
        output,
        dropped: outcome == FunctionOutcome::Dropped,
    }))
}

#[cfg(test)]
mod reader_language_tests {
    use serde_json::json;

    use super::*;
    use crate::shared::i18n_language_seed::seeded_pack;

    fn function(org_id: &str, name: &str, params_schema: Value) -> Function {
        Function {
            id: Id::from_string("function-1"),
            org_id: Id::from_string(org_id),
            name: name.into(),
            language: FunctionLanguage::Vrl,
            source: ". = parse_json!(.message)".into(),
            params_schema,
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(0),
        }
    }

    /// The name of every built-in function the baseline seeds.
    fn seeded_builtin_names() -> Vec<String> {
        let baseline = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../crates/engines/postgres/src/migrations/20260101000001_initial.sql"
        ))
        .expect("read the baseline migration");

        regex::Regex::new(r"(?m)^\('[0-9a-f-]{36}', '__builtin__', '([a-z0-9-]+)', '")
            .expect("valid seed row pattern")
            .captures_iter(&baseline)
            .map(|row| row[1].to_string())
            .collect()
    }

    #[test]
    fn a_builtin_function_is_described_in_the_readers_language() {
        let stored = json!({ "description": "Stored description." });

        let chinese = described(
            function("__builtin__", "redact-email", stored.clone()),
            &seeded_pack(BUILTIN_FUNCTION_KEY, "zh-cn"),
        );

        let english = described(
            function("__builtin__", "redact-email", stored),
            &seeded_pack(BUILTIN_FUNCTION_KEY, "en-us"),
        );

        assert!(chinese.is_builtin);
        assert_eq!(
            chinese.description,
            "把 message 中的邮箱地址替换为占位符（脱敏）。"
        );

        assert_eq!(
            english.description,
            "Replaces email addresses in message with a placeholder for redaction."
        );
    }

    #[test]
    fn a_function_someone_wrote_keeps_its_own_description() {
        let own = described(
            function("org-1", "redact-email", json!({ "description": "Ours." })),
            &seeded_pack(BUILTIN_FUNCTION_KEY, "zh-cn"),
        );

        let undescribed = described(
            function("org-1", "plain", json!({})),
            &seeded_pack(BUILTIN_FUNCTION_KEY, "zh-cn"),
        );

        assert!(!own.is_builtin);
        assert_eq!(own.description, "Ours.");
        assert_eq!(undescribed.description, "");
    }

    #[test]
    fn a_builtin_function_without_an_entry_keeps_its_stored_description() {
        let unread = described(
            function(
                "__builtin__",
                "redact-email",
                json!({ "description": "Stored description." }),
            ),
            &LanguagePack::default(),
        );

        assert_eq!(unread.description, "Stored description.");
    }

    #[test]
    fn every_seeded_builtin_function_has_a_description_entry() {
        let names = seeded_builtin_names();
        let descriptions = seeded_pack(BUILTIN_FUNCTION_KEY, "zh-cn");

        assert!(
            names.len() >= 5,
            "the built-in functions were not read: {names:?}"
        );

        for name in names {
            assert!(
                descriptions.get(&builtin_description_key(&name)).is_some(),
                "built-in function {name} has no description in the i18n_language seed"
            );
        }
    }
}
