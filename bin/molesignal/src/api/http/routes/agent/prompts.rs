// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Mole Agent 指令模板路由。
//!
//! 路径统一位于 `/api/v1/agent/settings/prompts`。
//!
//! builtin 行只读。变量校验：body 只能引用 `variables_schema` 允许的变量（task 2.4）。

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    routing::get,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    api::{
        AppState,
        http::{
            interface_language::interface_language_pack, middleware::Permission,
            routes::activity_audit,
        },
    },
    app::iam::IamContext,
    domain::{i18n_language::LanguagePack, iam::permission},
    infra::persistence::repositories::agent::prompts::{
        AgentPromptTemplate, prompt_hash, validate_template_variables,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/agent/settings/prompts", get(list).post(create))
        .route(
            "/agent/settings/prompts/{id}",
            axum::routing::put(update).delete(delete),
        )
        .route(
            "/agent/settings/prompts/{id}/set-default",
            axum::routing::post(set_default),
        )
        .route(
            "/agent/settings/prompts/{id}/restore",
            axum::routing::post(restore),
        )
}

/// The built-in prompts' names in the `i18n_language` table are under this
/// prefix: `agent.prompt.<builtin_key>.name`.
const BUILTIN_PROMPT_KEY: &str = "agent.prompt";

const PURPOSES: &[&str] = &[
    "system",
    "anomaly_analysis",
    "root_cause",
    "alert_explain",
    "query_generation",
    "dashboard_authoring",
];

fn validate_purpose(p: &str) -> Result<()> {
    if PURPOSES.contains(&p) {
        Ok(())
    } else {
        Err(Error::invalid(format!("unknown purpose: {p}")))
    }
}

/// scope-based 授权：org override 需 Admin+；user override 需 StreamWrite + 归属本人。
fn authorize_scope(ctx: &IamContext, scope: &str, owner_user_id: Option<&str>) -> Result<()> {
    match scope {
        "org" => Permission::require_key(ctx, "agent.manage"),
        "user" => {
            Permission::require_key(ctx, "agent.manage")?;

            if let Some(uid) = owner_user_id
                && uid != ctx.user_id.0
            {
                return Err(Error::forbidden("cannot modify another user's prompt"));
            }

            Ok(())
        }
        "builtin" => Err(Error::invalid("builtin prompts are immutable")),
        other => Err(Error::invalid(format!("unknown scope: {other}"))),
    }
}

#[derive(Debug, Serialize)]
pub struct PromptResp {
    pub id: String,
    pub org_id: Option<String>,
    pub user_id: Option<String>,
    pub scope: String,
    pub builtin_key: Option<String>,
    pub purpose: String,
    pub name: String,
    pub body: String,
    pub variables_schema: Value,
    pub is_default: bool,
    pub enabled: bool,
    pub version: i32,
    pub parent_id: Option<String>,
    pub created_at_micros: i64,
    pub updated_at_micros: i64,
}

fn to_resp(t: AgentPromptTemplate) -> PromptResp {
    PromptResp {
        id: t.id.0,
        org_id: t.org_id,
        user_id: t.user_id,
        scope: t.scope,
        builtin_key: t.builtin_key,
        purpose: t.purpose,
        name: t.name,
        body: t.body,
        variables_schema: t.variables_schema,
        is_default: t.is_default,
        enabled: t.enabled,
        version: t.version,
        parent_id: t.parent_id,
        created_at_micros: t.created_at.0,
        updated_at_micros: t.updated_at.0,
    }
}

#[derive(Debug, Deserialize)]
pub struct CreateReq {
    pub scope: String, // org | user
    pub purpose: String,
    #[serde(default)]
    pub builtin_key: Option<String>,
    pub name: String,
    pub body: String,
    #[serde(default)]
    pub variables_schema: Option<Value>,
    #[serde(default)]
    pub parent_id: Option<String>,
    #[serde(default)]
    pub enabled: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateReq {
    pub name: String,
    pub body: String,
    #[serde(default)]
    pub variables_schema: Option<Value>,
    #[serde(default)]
    pub enabled: Option<bool>,
}

#[permission("agent.use")]
async fn list(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Vec<PromptResp>>> {
    let rows = state.agent.prompts.list(&ctx.org_id, &ctx.user_id).await?;
    let names = interface_language_pack(&state, &ctx, BUILTIN_PROMPT_KEY).await;

    Ok(Json(
        rows.into_iter()
            .map(|row| in_reader_language(to_resp(row), &names))
            .collect(),
    ))
}

/// A built-in prompt under the name its key has in the reader's language.
///
/// A prompt that someone wrote keeps the name they gave it, an override of a
/// built-in one included. So does a built-in prompt the table has no entry for.
fn in_reader_language(mut prompt: PromptResp, names: &LanguagePack) -> PromptResp {
    if prompt.scope != "builtin" {
        return prompt;
    }

    let name = prompt
        .builtin_key
        .as_deref()
        .and_then(|key| names.get(&format!("{BUILTIN_PROMPT_KEY}.{key}.name")));

    if let Some(name) = name {
        prompt.name = name.to_string();
    }

    prompt
}

async fn create(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(req): Json<CreateReq>,
) -> Result<Json<PromptResp>> {
    validate_purpose(&req.purpose)?;

    if req.name.trim().is_empty() || req.body.trim().is_empty() {
        return Err(Error::invalid("name and body are required"));
    }

    // user scope 归属本人；org scope owner=None。
    authorize_scope(&ctx, &req.scope, Some(&ctx.user_id.0))?;

    let variables_schema = req
        .variables_schema
        .unwrap_or_else(|| json!({ "type": "object", "properties": {} }));

    // task 2.4：body 只能引用 variables_schema 允许的变量。
    validate_template_variables(&req.body, &variables_schema)?;

    let now = TimestampMicros::now();

    let user_id = if req.scope == "user" {
        Some(ctx.user_id.0.clone())
    } else {
        None
    };

    let id = Id::new();

    let t = AgentPromptTemplate {
        id: id.clone(),
        org_id: Some(ctx.org_id.0.clone()),
        user_id,
        scope: req.scope.clone(),
        builtin_key: req.builtin_key.clone(),
        purpose: req.purpose.clone(),
        name: req.name.clone(),
        body: req.body.clone(),
        variables_schema,
        is_default: false,
        enabled: req.enabled.unwrap_or(true),
        version: 1,
        parent_id: req.parent_id.clone(),
        created_by: Some(ctx.user_id.0.clone()),
        updated_by: Some(ctx.user_id.0.clone()),
        created_at: now,
        updated_at: now,
    };

    let saved = state.agent.prompts.create(t).await?;

    activity_audit::record(
        &state,
        &ctx,
        "agent.prompt.created",
        "agent_prompt",
        &saved.id.0,
        json!({
            "scope": saved.scope,
            "purpose": saved.purpose,
            "builtin_key": saved.builtin_key,
            "version": saved.version,
            "prompt_hash": prompt_hash(&saved.body),
        }),
    )
    .await;

    Ok(Json(to_resp(saved)))
}

async fn update(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(req): Json<UpdateReq>,
) -> Result<Json<PromptResp>> {
    if req.name.trim().is_empty() || req.body.trim().is_empty() {
        return Err(Error::invalid("name and body are required"));
    }

    let existing = state.agent.prompts.get(&Id(id.clone())).await?;
    authorize_scope(&ctx, &existing.scope, existing.user_id.as_deref())?;

    // org override 也要校验本 org。
    if existing.org_id.as_deref() != Some(ctx.org_id.0.as_str()) {
        return Err(Error::forbidden("prompt belongs to another org"));
    }

    let variables_schema = req
        .variables_schema
        .unwrap_or(existing.variables_schema.clone());

    validate_template_variables(&req.body, &variables_schema)?;

    let mut next = existing.clone();
    next.name = req.name.clone();
    next.body = req.body.clone();
    next.variables_schema = variables_schema;
    next.enabled = req.enabled.unwrap_or(existing.enabled);
    next.updated_by = Some(ctx.user_id.0.clone());
    let saved = state.agent.prompts.update(next).await?;

    activity_audit::record(
        &state,
        &ctx,
        "agent.prompt.updated",
        "agent_prompt",
        &saved.id.0,
        json!({
            "scope": saved.scope,
            "purpose": saved.purpose,
            "version": saved.version,
            "prompt_hash": prompt_hash(&saved.body),
        }),
    )
    .await;

    Ok(Json(to_resp(saved)))
}

async fn set_default(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<PromptResp>> {
    let existing = state.agent.prompts.get(&Id(id.clone())).await?;
    authorize_scope(&ctx, &existing.scope, existing.user_id.as_deref())?;

    if existing.org_id.as_deref() != Some(ctx.org_id.0.as_str()) {
        return Err(Error::forbidden("prompt belongs to another org"));
    }

    state.agent.prompts.set_default(&Id(id.clone())).await?;

    activity_audit::record(
        &state,
        &ctx,
        "agent.prompt.default_set",
        "agent_prompt",
        &id,
        json!({ "scope": existing.scope, "purpose": existing.purpose }),
    )
    .await;

    let saved = state.agent.prompts.get(&Id(id)).await?;
    Ok(Json(to_resp(saved)))
}

async fn restore(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<PromptResp>> {
    let existing = state.agent.prompts.get(&Id(id.clone())).await?;
    authorize_scope(&ctx, &existing.scope, existing.user_id.as_deref())?;

    if existing.org_id.as_deref() != Some(ctx.org_id.0.as_str()) {
        return Err(Error::forbidden("prompt belongs to another org"));
    }

    let builtin_key = existing
        .builtin_key
        .clone()
        .ok_or_else(|| Error::invalid("override has no builtin parent to restore from"))?;

    let builtin = state.agent.prompts.get_builtin(&builtin_key).await?;
    // 用 builtin body/schema 覆盖 override，递增 version。
    let mut next = existing.clone();
    next.body = builtin.body;
    next.variables_schema = builtin.variables_schema;
    next.updated_by = Some(ctx.user_id.0.clone());
    let saved = state.agent.prompts.update(next).await?;

    activity_audit::record(
        &state,
        &ctx,
        "agent.prompt.restored",
        "agent_prompt",
        &saved.id.0,
        json!({
            "scope": saved.scope,
            "purpose": saved.purpose,
            "version": saved.version,
            "restored_from_builtin": builtin_key,
            "prompt_hash": prompt_hash(&saved.body),
        }),
    )
    .await;

    Ok(Json(to_resp(saved)))
}

async fn delete(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    let existing = state.agent.prompts.get(&Id(id.clone())).await?;
    authorize_scope(&ctx, &existing.scope, existing.user_id.as_deref())?;

    if existing.org_id.as_deref() != Some(ctx.org_id.0.as_str()) {
        return Err(Error::forbidden("prompt belongs to another org"));
    }

    state.agent.prompts.delete(&Id(id.clone())).await?;

    activity_audit::record(
        &state,
        &ctx,
        "agent.prompt.deleted",
        "agent_prompt",
        &id,
        json!({ "scope": existing.scope, "purpose": existing.purpose }),
    )
    .await;

    Ok(Json(json!({ "deleted": true })))
}

#[cfg(test)]
mod reader_language_tests {
    use super::*;
    use crate::shared::i18n_language_seed::seeded_pack;

    fn prompt(scope: &str, builtin_key: Option<&str>, name: &str) -> PromptResp {
        PromptResp {
            id: "prompt-1".into(),
            org_id: None,
            user_id: None,
            scope: scope.into(),
            builtin_key: builtin_key.map(str::to_string),
            purpose: "anomaly_analysis".into(),
            name: name.into(),
            body: "body".into(),
            variables_schema: json!({}),
            is_default: true,
            enabled: true,
            version: 1,
            parent_id: None,
            created_at_micros: 0,
            updated_at_micros: 0,
        }
    }

    /// The `builtin_key` of every built-in prompt the baseline seeds.
    fn seeded_builtin_keys() -> Vec<String> {
        let baseline = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../crates/engines/postgres/src/migrations/20260101000001_initial.sql"
        ))
        .expect("read the baseline migration");

        let (_, seed) = baseline
            .split_once("INSERT INTO ai_prompt_templates")
            .expect("built-in prompt seed");

        let seed = &seed[..seed.find("ON CONFLICT").expect("end of the seed")];

        regex::Regex::new(r"NULL, NULL, 'builtin', '([a-z0-9_.]+)'")
            .expect("valid seed row pattern")
            .captures_iter(seed)
            .map(|row| row[1].to_string())
            .collect()
    }

    #[test]
    fn a_builtin_prompt_is_named_in_the_readers_language() {
        let stored = prompt("builtin", Some("analysis.anomaly"), "Anomaly Analysis");

        let chinese = in_reader_language(
            prompt("builtin", Some("analysis.anomaly"), "Anomaly Analysis"),
            &seeded_pack(BUILTIN_PROMPT_KEY, "zh-cn"),
        );

        let english = in_reader_language(stored, &seeded_pack(BUILTIN_PROMPT_KEY, "en-us"));

        assert_eq!(chinese.name, "异常分析");
        assert_eq!(english.name, "Anomaly Analysis");
    }

    #[test]
    fn a_prompt_someone_wrote_keeps_its_name() {
        let names = seeded_pack(BUILTIN_PROMPT_KEY, "zh-cn");

        let overridden = in_reader_language(
            prompt("org", Some("analysis.anomaly"), "Our anomaly playbook"),
            &names,
        );

        let personal = in_reader_language(prompt("user", None, "Mine"), &names);

        assert_eq!(overridden.name, "Our anomaly playbook");
        assert_eq!(personal.name, "Mine");
    }

    #[test]
    fn a_builtin_prompt_without_an_entry_keeps_its_stored_name() {
        let unnamed = in_reader_language(
            prompt("builtin", Some("not.seeded"), "Stored name"),
            &seeded_pack(BUILTIN_PROMPT_KEY, "zh-cn"),
        );

        let unread = in_reader_language(
            prompt("builtin", Some("analysis.anomaly"), "Anomaly Analysis"),
            &LanguagePack::default(),
        );

        assert_eq!(unnamed.name, "Stored name");
        assert_eq!(unread.name, "Anomaly Analysis");
    }

    #[test]
    fn every_seeded_builtin_prompt_has_a_name_entry() {
        let keys = seeded_builtin_keys();
        let names = seeded_pack(BUILTIN_PROMPT_KEY, "zh-cn");

        assert!(
            keys.len() >= 6,
            "the built-in prompts were not read: {keys:?}"
        );

        for key in keys {
            assert!(
                names
                    .get(&format!("{BUILTIN_PROMPT_KEY}.{key}.name"))
                    .is_some(),
                "built-in prompt {key} has no name in the i18n_language seed"
            );
        }
    }
}
