// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `POST /agent/knowledge/upload`：上传文本文件或整个文件夹。
//!
//! 每个文件一个 `file` 字段，文件名是相对路径（上传文件夹时带目录）；`dir` 是放到哪个目录下。
//! 不是 UTF-8 文本的、以 `.` 开头的、太大的文件跳过并说明原因；其余的一起写，超出这份知识的
//! 份额时一个都不写。

use axum::{
    Extension, Json,
    extract::{Multipart, State},
};
use serde_json::{Value, json};

use super::{audit, editable, knowledge_scope};
use crate::{
    agent::knowledge::{MAX_FILE_BYTES, normalize_path},
    api::AppState,
    app::iam::IamContext,
    domain::iam::permission,
    shared::{Error, Result},
};

#[permission("agent.use")]
pub(super) async fn upload(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    mut multipart: Multipart,
) -> Result<Json<Value>> {
    let mut scope = String::new();
    let mut team_id: Option<String> = None;
    let mut dir = String::new();
    let mut received: Vec<(String, Vec<u8>)> = Vec::new();

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|error| Error::invalid(format!("multipart: {error}")))?
    {
        let name = field.name().unwrap_or_default().to_string();

        if name == "file" {
            let path = field.file_name().unwrap_or_default().to_string();

            let bytes = field
                .bytes()
                .await
                .map_err(|error| Error::invalid(format!("file {path}: {error}")))?;

            received.push((path, bytes.to_vec()));
            continue;
        }

        let value = field
            .text()
            .await
            .map_err(|error| Error::invalid(format!("field {name}: {error}")))?;

        match name.as_str() {
            "scope" => scope = value,
            "team_id" => team_id = Some(value).filter(|value| !value.trim().is_empty()),
            "dir" => dir = value,
            _ => {}
        }
    }

    let (scope, viewer) = knowledge_scope(&state, &ctx, &scope, team_id.as_deref()).await?;
    editable(&viewer, &scope)?;

    if received.is_empty() {
        return Err(Error::invalid("choose the files to upload"));
    }

    let dir = dir.trim().trim_matches('/');
    let mut files = Vec::new();
    let mut skipped = Vec::new();

    for (raw, bytes) in received {
        let joined = if dir.is_empty() {
            raw.clone()
        } else {
            format!("{dir}/{raw}")
        };

        let path = match normalize_path(&joined) {
            Ok(path) => path,
            Err(_) => {
                skipped.push(json!({ "path": raw, "reason": "hidden_or_invalid_path" }));
                continue;
            }
        };

        if bytes.len() > MAX_FILE_BYTES {
            skipped.push(json!({ "path": path, "reason": "too_large" }));
            continue;
        }

        match String::from_utf8(bytes) {
            Ok(text) if !text.contains('\0') => files.push((path, text)),
            _ => skipped.push(json!({ "path": path, "reason": "not_text" })),
        }
    }

    if !files.is_empty() {
        state
            .agent
            .knowledge
            .put(&ctx.org_id, &scope, &files, &ctx.user_id)
            .await?;

        audit(
            &state,
            &ctx,
            &scope,
            "uploaded",
            json!({ "files": files.len(), "skipped": skipped.len() }),
        )
        .await;
    }

    Ok(Json(json!({ "written": files.len(), "skipped": skipped })))
}
