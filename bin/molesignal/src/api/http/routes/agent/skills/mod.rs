// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Mole Agent Skill 的管理接口。
//!
//! - `GET    /api/v1/agent/skills`                 看得到的 Skill，加实例目录与扫描问题
//! - `POST   /api/v1/agent/skills`                 上传一个包（multipart：file、scope、team_id、replace）
//! - `GET    /api/v1/agent/skills/{id}`            正文与文件清单
//! - `GET    /api/v1/agent/skills/{id}/file?path=` 一个文本文件
//! - `GET    /api/v1/agent/skills/{id}/package`    下载：上传的原包，其他来源打成 zip
//! - `PATCH  /api/v1/agent/skills/{id}`            启停、移交作用域
//! - `DELETE /api/v1/agent/skills/{id}`            删除上传的 Skill
//! - `POST   /api/v1/agent/skills/rescan`          重扫实例目录（`agent.manage`）
//!
//! 仓库来源在 [`repositories`]。谁能改什么按 `agent::scope` 判定；安装、替换、启停、移交、
//! 删除都进审计。

mod repositories;

use axum::{
    Extension, Json, Router,
    body::Body,
    extract::{DefaultBodyLimit, Multipart, Path, Query, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};

use super::viewer::{requested_scope, viewer};
use crate::{
    agent::skills::MAX_PACKAGE_BYTES,
    api::{
        AppState,
        http::{interface_language::interface_language_pack, routes::activity_audit},
    },
    app::{
        iam::IamContext,
        skills::{BUNDLED_SKILL_KEY, InstallRequest, Installed},
    },
    domain::iam::permission,
    shared::{Error, Result},
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/agent/skills",
            get(list_skills)
                .post(upload_skill)
                .layer(DefaultBodyLimit::max(MAX_PACKAGE_BYTES + 64 * 1024)),
        )
        .route("/agent/skills/rescan", post(rescan))
        .route(
            "/agent/skills/{id}",
            get(skill_detail).patch(update_skill).delete(delete_skill),
        )
        .route("/agent/skills/{id}/file", get(skill_file))
        .route("/agent/skills/{id}/package", get(skill_package))
        .merge(repositories::routes())
}

#[permission("agent.use")]
async fn list_skills(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Value>> {
    let viewer = viewer(&state, &ctx).await?;
    let texts = interface_language_pack(&state, &ctx, BUNDLED_SKILL_KEY).await;

    let skills = state
        .agent
        .skills
        .list(&ctx.org_id, &viewer)
        .await?
        .into_iter()
        .map(|skill| skill.localized(&texts))
        .collect::<Vec<_>>();

    let instance = state.agent.skills.instance();

    Ok(Json(json!({
        "skills": skills,
        "paths": {
            "roots": instance.roots(),
            "problems": if viewer.manager { json!(instance.problems()) } else { json!([]) },
            "scanned_at_micros": instance.scanned_at().map(|at| at.0),
        },
    })))
}

#[permission("agent.use")]
async fn upload_skill(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    mut multipart: Multipart,
) -> Result<Response> {
    let mut file: Option<(String, Vec<u8>)> = None;
    let mut scope = "personal".to_string();
    let mut team_id: Option<String> = None;
    let mut replace = false;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|error| Error::invalid(format!("multipart: {error}")))?
    {
        let name = field.name().unwrap_or_default().to_string();

        if name == "file" {
            let file_name = field.file_name().unwrap_or("skill.zip").to_string();

            let bytes = field
                .bytes()
                .await
                .map_err(|error| Error::invalid(format!("skill package: {error}")))?;

            file = Some((file_name, bytes.to_vec()));
            continue;
        }

        let value = field
            .text()
            .await
            .map_err(|error| Error::invalid(format!("field {name}: {error}")))?;

        match name.as_str() {
            "scope" => scope = value,
            "team_id" => team_id = Some(value),
            "replace" => replace = matches!(value.as_str(), "true" | "1"),
            _ => {}
        }
    }

    let (file_name, bytes) = file.ok_or_else(|| Error::invalid("choose a package to upload"))?;
    let scope = requested_scope(&state, &ctx, &scope, team_id.as_deref()).await?;
    let viewer = viewer(&state, &ctx).await?;

    let installed = state
        .agent
        .skills
        .install(
            &ctx.org_id,
            &viewer,
            InstallRequest {
                scope,
                file_name,
                bytes,
                replace,
            },
        )
        .await?;

    let skill = match installed {
        Installed::Saved(skill) => skill,
        Installed::Exists { name } => {
            return Ok((
                StatusCode::CONFLICT,
                Json(json!({
                    "error": "skill_exists",
                    "code": "skill_exists",
                    "name": name,
                    "message": format!("a skill named `{name}` already exists in this scope"),
                })),
            )
                .into_response());
        }
    };

    activity_audit::record(
        &state,
        &ctx,
        if replace {
            "agent.skill.replaced"
        } else {
            "agent.skill.installed"
        },
        "agent_skill",
        &skill.id.0,
        json!({
            "name": skill.name,
            "scope": skill.scope.kind(),
            "version": skill.version,
            "hash": skill.content_hash,
        }),
    )
    .await;

    Ok(Json(skill).into_response())
}

#[permission("agent.use")]
async fn skill_detail(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    let viewer = viewer(&state, &ctx).await?;
    let texts = interface_language_pack(&state, &ctx, BUNDLED_SKILL_KEY).await;
    let mut detail = state.agent.skills.detail(&ctx.org_id, &viewer, &id).await?;
    detail.view = detail.view.localized(&texts);

    Ok(Json(json!(detail)))
}

#[derive(Debug, Deserialize)]
struct FileQuery {
    path: String,
}

#[permission("agent.use")]
async fn skill_file(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Query(query): Query<FileQuery>,
) -> Result<Json<Value>> {
    let viewer = viewer(&state, &ctx).await?;

    let text = state
        .agent
        .skills
        .file(&ctx.org_id, &viewer, &id, &query.path)
        .await?;

    Ok(Json(json!({ "path": query.path, "content": text })))
}

#[permission("agent.use")]
async fn skill_package(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Response> {
    let viewer = viewer(&state, &ctx).await?;
    let (name, bytes) = state
        .agent
        .skills
        .package(&ctx.org_id, &viewer, &id)
        .await?;
    let safe_name: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_graphic() && c != '"' && c != '\\' {
                c
            } else {
                '_'
            }
        })
        .collect();

    Ok((
        [
            (header::CONTENT_TYPE, "application/octet-stream".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{safe_name}\""),
            ),
        ],
        Body::from(bytes),
    )
        .into_response())
}

#[derive(Debug, Deserialize)]
struct UpdateSkillRequest {
    #[serde(default)]
    enabled: Option<bool>,
    /// 移交到这个作用域。
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    team_id: Option<String>,
}

#[permission("agent.use")]
async fn update_skill(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(request): Json<UpdateSkillRequest>,
) -> Result<Json<Value>> {
    let viewer = viewer(&state, &ctx).await?;

    if let Some(kind) = request.scope.as_deref() {
        let target = requested_scope(&state, &ctx, kind, request.team_id.as_deref()).await?;

        let moved = state
            .agent
            .skills
            .move_to(&ctx.org_id, &viewer, &id, target)
            .await?;

        activity_audit::record(
            &state,
            &ctx,
            "agent.skill.moved",
            "agent_skill",
            &moved.id.0,
            json!({ "name": moved.name, "scope": moved.scope.kind() }),
        )
        .await;
    }

    if let Some(enabled) = request.enabled {
        state
            .agent
            .skills
            .set_enabled(&ctx.org_id, &viewer, &id, enabled)
            .await?;

        activity_audit::record(
            &state,
            &ctx,
            if enabled {
                "agent.skill.enabled"
            } else {
                "agent.skill.disabled"
            },
            "agent_skill",
            &id,
            json!({}),
        )
        .await;
    }

    let detail = state.agent.skills.detail(&ctx.org_id, &viewer, &id).await?;
    Ok(Json(json!(detail)))
}

#[permission("agent.use")]
async fn delete_skill(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    let viewer = viewer(&state, &ctx).await?;
    let skill = state.agent.skills.delete(&ctx.org_id, &viewer, &id).await?;

    activity_audit::record(
        &state,
        &ctx,
        "agent.skill.deleted",
        "agent_skill",
        &skill.id.0,
        json!({ "name": skill.name, "scope": skill.scope.kind() }),
    )
    .await;

    Ok(Json(json!({ "deleted": true })))
}

#[permission("agent.manage")]
async fn rescan(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
) -> Result<Json<Value>> {
    let instance = state.agent.skills.instance().clone();

    tokio::task::spawn_blocking(move || instance.rescan(true))
        .await
        .map_err(|error| Error::internal(format!("skill folder scan: {error}")))?;

    let instance = state.agent.skills.instance();

    Ok(Json(json!({
        "roots": instance.roots(),
        "problems": instance.problems(),
        "scanned_at_micros": instance.scanned_at().map(|at| at.0),
    })))
}
