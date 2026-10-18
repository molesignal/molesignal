// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 会话 transcript 归档：对象存储里一份 JSON，PG 里一行归档元数据，再加一条审计。

use std::collections::BTreeMap;

use axum::{
    Extension, Json,
    extract::{Path, State},
};
use object_store::{ObjectStoreExt, PutPayload, path::Path as ObjPath};
use serde_json::{Value, json};

use super::{super::viewer::readable_chat, CHAT_OBJECT_LAYOUT_VERSION};
use crate::{
    api::{AppState, http::routes::activity_audit},
    app::iam::IamContext,
    domain::iam::permission,
    infra::persistence::repositories::agent::{chats::Chat, prompts::prompt_hash},
    shared::{Result, ids::Id, time::TimestampMicros},
};

const CHAT_TRANSCRIPT_SCHEMA_VERSION: u32 = 1;

#[permission("agent.use")]
pub(super) async fn archive_chat_route(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    let (chat, _) = readable_chat(&state, &ctx, &Id(id)).await?;
    let archive = archive_chat(&state, &ctx, &chat).await;

    Ok(Json(json!({
        "status": archive.status,
        "object_key": archive.object_key,
        "sha256": archive.sha256,
        "bytes": archive.bytes,
        "error": archive.error,
    })))
}

/// 写 transcript JSON 到对象存储 + PG 归档元数据 + 审计；失败也记一行（status=failed），
/// 绝不抹掉 PG chat history，审计 payload 不含 transcript 原文。
pub(super) async fn archive_chat(
    state: &AppState,
    ctx: &IamContext,
    chat: &Chat,
) -> crate::infra::persistence::repositories::agent::chat_archives::ChatArchive {
    use crate::infra::persistence::repositories::agent::chat_archives::ChatArchive;

    let messages = state
        .agent
        .chats
        .list_messages(&chat.id)
        .await
        .unwrap_or_default();

    let total_prompt: i64 = messages.iter().filter_map(|m| m.prompt_tokens).sum();
    let total_completion: i64 = messages.iter().filter_map(|m| m.completion_tokens).sum();
    // 不同币种的成本不能相加：每个币种一个合计。
    let mut cost_totals: BTreeMap<String, f64> = BTreeMap::new();
    for cost in messages.iter().filter_map(|m| m.cost) {
        *cost_totals
            .entry(cost.currency.as_str().to_string())
            .or_default() += cost.amount;
    }
    let archived_at = TimestampMicros::now();

    let transcript = json!({
        "schema_version": CHAT_TRANSCRIPT_SCHEMA_VERSION,
        "chat": {
            "id": chat.id.0,
            "org_id": chat.org_id.0,
            "provider": chat.provider,
            "provider_id": chat.provider_id,
            "model": chat.model,
            "title": chat.title,
            "analysis_mode": chat.analysis_mode,
            "scope": chat.scope,
            "team_id": chat.team_id,
            "time_range_start_micros": chat.time_range_start_micros,
            "time_range_end_micros": chat.time_range_end_micros,
            "created_at_micros": chat.created_at.0,
            "updated_at_micros": chat.updated_at.0,
        },
        "messages": messages.iter().map(|m| json!({
            "id": m.id.0,
            "role": m.role,
            "user_id": m.user_id,
            "content": m.content,
            "prompt_template_id": m.prompt_template_id,
            "prompt_builtin_key": m.prompt_builtin_key,
            "prompt_version": m.prompt_version,
            "prompt_hash": m.prompt_hash,
            "evidence": m.evidence_json,
            "command": m.command_json,
            "prompt_tokens": m.prompt_tokens,
            "completion_tokens": m.completion_tokens,
            "cost": m.cost,
            "created_at_micros": m.created_at.0,
        })).collect::<Vec<_>>(),
        "token_usage": { "prompt_tokens": total_prompt, "completion_tokens": total_completion },
        "cost_totals": cost_totals,
        "archived_at_micros": archived_at.0,
    });

    let body = serde_json::to_string(&transcript).unwrap_or_else(|_| "{}".into());
    let sha = prompt_hash(&body);
    let bytes = body.len() as i64;
    let object_key = transcript_object_key(&chat.org_id, &chat.id, archived_at);

    let write_ok = match ObjPath::parse(&object_key) {
        Ok(path) => state
            .storage
            .object_store
            .put(&path, PutPayload::from(body.into_bytes()))
            .await
            .map_err(|e| e.to_string()),
        Err(e) => Err(e.to_string()),
    };

    let archive = match write_ok {
        Ok(_) => {
            let _ = state
                .agent
                .chats
                .set_archive_object_key(&chat.id, &object_key)
                .await;

            ChatArchive {
                id: Id::new(),
                chat_id: chat.id.clone(),
                org_id: chat.org_id.clone(),
                object_key: Some(object_key.clone()),
                sha256: Some(sha.clone()),
                bytes,
                status: "ok".into(),
                error: None,
                created_by: Some(ctx.user_id.0.clone()),
                created_at: TimestampMicros::now(),
            }
        }
        Err(err) => {
            tracing::warn!(error = %err, chat = %chat.id.0, "chat archive write failed");

            ChatArchive {
                id: Id::new(),
                chat_id: chat.id.clone(),
                org_id: chat.org_id.clone(),
                object_key: None,
                sha256: None,
                bytes: 0,
                status: "failed".into(),
                error: Some(err),
                created_by: Some(ctx.user_id.0.clone()),
                created_at: TimestampMicros::now(),
            }
        }
    };

    let saved = state
        .agent
        .chat_archives
        .record(archive.clone())
        .await
        .unwrap_or(archive);

    // 审计：含 object_key / checksum / status，不含 transcript 原文。
    activity_audit::record(
        state,
        ctx,
        "agent.chat.archived",
        "agent_chat",
        &chat.id.0,
        json!({
            "status": saved.status,
            "object_key": saved.object_key,
            "sha256": saved.sha256,
            "bytes": saved.bytes,
            "error": saved.error,
        }),
    )
    .await;

    saved
}

fn transcript_object_key(org_id: &Id, chat_id: &Id, archived_at: TimestampMicros) -> String {
    format!(
        "agent/chat/{CHAT_OBJECT_LAYOUT_VERSION}/{}/{}/transcript-{}.json",
        org_id.0, chat_id.0, archived_at.0
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transcript_object_keys_include_layout_version() {
        assert_eq!(
            transcript_object_key(
                &Id::from_string("org-1"),
                &Id::from_string("chat-1"),
                TimestampMicros(42)
            ),
            "agent/chat/v1/org-1/chat-1/transcript-42.json"
        );
    }
}
