// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 对 Mole Agent 产出的 👍 / 👎。
//!
//! - `PUT    /api/v1/agent/feedback`                       保存自己对一个对象的评价
//! - `DELETE /api/v1/agent/feedback/{target_kind}/{target_id}` 撤回自己的评价
//! - `GET    /api/v1/agent/feedback?target_kind=&target_ids=` 自己对这些对象的评价
//!
//! 对话回答只有问这个问题的人能评价（团队会话里也是）；事件 RCA 组织里能用 Mole Agent 的人
//! 都能评价。评价时记下被
//! 评价的回答当时用的模型与 prompt 版本。

use axum::{
    Extension, Json, Router,
    extract::{Path, Query, State},
    routing::{delete, get},
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    agent::feedback::{
        AgentFeedback, FeedbackRating, FeedbackReason, FeedbackTarget, MAX_TARGETS_PER_LOOKUP,
    },
    api::{
        AppState,
        http::routes::{activity_audit, agent::viewer::readable_chat},
    },
    app::iam::IamContext,
    domain::iam::permission,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/agent/feedback", get(list_feedback).put(put_feedback))
        .route(
            "/agent/feedback/{target_kind}/{target_id}",
            delete(delete_feedback),
        )
}

#[derive(Debug, Deserialize)]
struct FeedbackRequest {
    target_kind: String,
    target_id: String,
    /// 对话回答所在的会话；评价对话回答时必填。
    #[serde(default)]
    chat_id: Option<String>,
    rating: String,
    #[serde(default)]
    reasons: Vec<String>,
    #[serde(default)]
    comment: String,
}

#[derive(Debug, Deserialize)]
struct FeedbackQuery {
    target_kind: String,
    /// 逗号分隔的对象 ID。
    #[serde(default)]
    target_ids: String,
}

/// 对象存在、调用者能评价它，并取出它当时用的模型与 prompt。
async fn provenance(
    state: &AppState,
    ctx: &IamContext,
    target_kind: FeedbackTarget,
    target_id: &str,
    chat_id: Option<&str>,
) -> Result<Value> {
    match target_kind {
        FeedbackTarget::ChatMessage => {
            let chat_id = chat_id
                .filter(|id| !id.trim().is_empty())
                .ok_or_else(|| Error::invalid("chat_id is required to rate a chat answer"))?;

            let (chat, _) = readable_chat(state, ctx, &Id(chat_id.to_string())).await?;
            let messages = state.agent.chats.list_messages(&chat.id).await?;

            let position = messages
                .iter()
                .position(|message| message.id.0 == target_id && message.role == "assistant")
                .ok_or_else(|| Error::not_found(format!("answer `{target_id}` not found")))?;

            // 评价的人是问这个问题的人：团队会话里每个问题可能来自不同的成员。
            let asker = messages[..position]
                .iter()
                .rev()
                .find(|message| message.role == "user")
                .and_then(|question| question.user_id.clone())
                .unwrap_or_else(|| chat.user_id.clone());

            if asker != ctx.user_id {
                return Err(Error::forbidden(
                    "only the person who asked can rate this answer",
                ));
            }

            let message = &messages[position];

            Ok(json!({
                "chat_id": chat.id.0,
                "provider": chat.provider,
                "provider_id": chat.provider_id,
                "model": chat.model,
                "analysis_mode": chat.analysis_mode,
                "prompt_template_id": message.prompt_template_id,
                "prompt_builtin_key": message.prompt_builtin_key,
                "prompt_version": message.prompt_version,
                "prompt_hash": message.prompt_hash,
                "prompt_tokens": message.prompt_tokens,
                "completion_tokens": message.completion_tokens,
                "cost": message.cost,
            }))
        }
        FeedbackTarget::IncidentRca => {
            let rca = state
                .agent
                .incident_rca
                .get(&Id(target_id.to_string()))
                .await?
                .filter(|rca| rca.org_id == ctx.org_id)
                .ok_or_else(|| {
                    Error::not_found(format!("root cause analysis for `{target_id}` not found"))
                })?;

            Ok(json!({
                "incident_id": rca.incident_id.0,
                "provider": rca.provider,
                "model": rca.model,
                "prompt_builtin_key": rca.prompt_builtin_key,
                "prompt_hash": rca.prompt_hash,
                "prompt_tokens": rca.prompt_tokens,
                "completion_tokens": rca.completion_tokens,
                "finish_reason": rca.finish_reason,
                "generated_at_micros": rca.updated_at.0,
            }))
        }
    }
}

#[permission("agent.use")]
async fn put_feedback(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(request): Json<FeedbackRequest>,
) -> Result<Json<AgentFeedback>> {
    let target_kind = FeedbackTarget::parse(&request.target_kind)?;
    let rating = FeedbackRating::parse(&request.rating)?;

    let reasons = request
        .reasons
        .iter()
        .map(|reason| FeedbackReason::parse(reason))
        .collect::<Result<Vec<_>>>()?;

    let provenance = provenance(
        &state,
        &ctx,
        target_kind,
        &request.target_id,
        request.chat_id.as_deref(),
    )
    .await?;

    let now = TimestampMicros::now();

    let feedback = AgentFeedback {
        id: Id::new(),
        org_id: ctx.org_id.clone(),
        target_kind,
        target_id: request.target_id.trim().to_string(),
        user_id: ctx.user_id.clone(),
        rating,
        reasons,
        comment: request.comment,
        provenance,
        created_at: now,
        updated_at: now,
    }
    .normalized()?;

    let saved = state.agent.feedback.upsert(feedback).await?;

    // 审计只记评分与原因，备注可能含业务细节，不进审计。
    activity_audit::record(
        &state,
        &ctx,
        "agent.feedback.submitted",
        "agent_feedback",
        &saved.target_id,
        json!({
            "target_kind": saved.target_kind,
            "rating": saved.rating,
            "reasons": saved.reasons,
            "has_comment": !saved.comment.is_empty(),
        }),
    )
    .await;

    Ok(Json(saved))
}

#[permission("agent.use")]
async fn delete_feedback(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path((target_kind, target_id)): Path<(String, String)>,
) -> Result<Json<Value>> {
    let target_kind = FeedbackTarget::parse(&target_kind)?;

    let removed = state
        .agent
        .feedback
        .remove(&ctx.org_id, target_kind, &target_id, &ctx.user_id)
        .await?;

    if removed {
        activity_audit::record(
            &state,
            &ctx,
            "agent.feedback.withdrawn",
            "agent_feedback",
            &target_id,
            json!({ "target_kind": target_kind }),
        )
        .await;
    }

    Ok(Json(json!({ "removed": removed })))
}

#[permission("agent.use")]
async fn list_feedback(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(query): Query<FeedbackQuery>,
) -> Result<Json<Value>> {
    let target_kind = FeedbackTarget::parse(&query.target_kind)?;
    let target_ids = split_ids(&query.target_ids)?;

    let feedback = state
        .agent
        .feedback
        .list_for_user(&ctx.org_id, target_kind, &target_ids, &ctx.user_id)
        .await?;

    Ok(Json(json!({ "feedback": feedback })))
}

fn split_ids(raw: &str) -> Result<Vec<String>> {
    let mut ids: Vec<String> = raw
        .split(',')
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_string)
        .collect();

    ids.sort();
    ids.dedup();

    if ids.len() > MAX_TARGETS_PER_LOOKUP {
        return Err(Error::invalid(format!(
            "at most {MAX_TARGETS_PER_LOOKUP} target ids per request"
        )));
    }

    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_trimmed_deduplicated_and_capped() {
        assert_eq!(split_ids(" b, a ,,b ").unwrap(), vec!["a", "b"]);
        assert!(split_ids("").unwrap().is_empty());

        let many = (0..=MAX_TARGETS_PER_LOOKUP)
            .map(|index| index.to_string())
            .collect::<Vec<_>>()
            .join(",");

        assert!(split_ids(&many).is_err());
    }
}
