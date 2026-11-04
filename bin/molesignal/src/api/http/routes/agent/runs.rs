// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 一轮回答（Run）的 HTTP 路由。
//!
//! - `GET  /api/v1/agent/runs/{id}`          状态
//! - `GET  /api/v1/agent/runs/{id}/events`   事件（SSE），`?after=序号` 或 `Last-Event-ID` 续读
//! - `POST /api/v1/agent/runs/{id}/cancel`   停止
//! - `POST /api/v1/agent/runs/{id}/respond`  回答问题、决定一个确认，或在存好凭据后继续
//! - `GET  /api/v1/agent/chat/{id}/runs`     会话最近的 Run，新的在前
//!
//! 能读会话的人能看它的 Run、停止它、回答它的问题；确认只能由发起这一轮的人决定，工具以他的
//! 身份执行；「授权以继续」也只有他能回，凭据是他自己的。

use axum::{
    Extension, Json, Router,
    extract::{Path, Query, State},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{chat::run::request::RunRequest, viewer::readable_chat};
use crate::{
    agent::{
        runs::{Pending, QuestionKind, Run, RunResponse},
        usage::Cost,
    },
    api::{AppState, http::routes::activity_audit},
    app::iam::IamContext,
    domain::iam::permission,
    shared::{Error, Result, ids::Id},
};

mod stream;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/agent/runs/{id}", get(get_run))
        .route("/agent/runs/{id}/events", get(stream::events))
        .route("/agent/runs/{id}/cancel", post(cancel_run))
        .route("/agent/runs/{id}/respond", post(respond))
        .route("/agent/chat/{id}/runs", get(chat_runs))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RunResp {
    pub id: String,
    pub chat_id: String,
    /// queued | running | waiting_input | waiting_approval | succeeded | failed | cancelled | interrupted
    pub status: String,
    pub source: String,
    /// answer | compact | distill：对话里只显示回答。
    pub kind: String,
    /// 发起这一轮的人。
    pub initiator_id: String,
    pub user_message_id: Option<String>,
    pub assistant_message_id: Option<String>,
    /// 在等的问题、确认或授权。
    pub pending: Option<Value>,
    pub cancel_requested: bool,
    /// 最后一个事件的序号：续读从它之后开始。
    pub last_seq: i64,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub cost: Option<Cost>,
    pub error: Option<String>,
    pub created_at: i64,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
}

impl RunResp {
    pub(crate) fn of(run: &Run) -> Self {
        Self {
            id: run.id.0.clone(),
            chat_id: run.chat_id.0.clone(),
            status: run.status.as_str().into(),
            source: run.source.as_str().into(),
            kind: RunRequest::kind_of(&run.request).into(),
            initiator_id: run.initiator().0.clone(),
            user_message_id: run.user_message_id.as_ref().map(|id| id.0.clone()),
            assistant_message_id: run.assistant_message_id.as_ref().map(|id| id.0.clone()),
            pending: run
                .pending
                .as_ref()
                .and_then(|pending| serde_json::to_value(pending).ok()),
            cancel_requested: run.cancel_requested,
            last_seq: run.last_seq,
            prompt_tokens: run.prompt_tokens,
            completion_tokens: run.completion_tokens,
            cost: run.cost,
            error: run.error.clone(),
            created_at: run.created_at.0,
            started_at: run.started_at.map(|at| at.0),
            finished_at: run.finished_at.map(|at| at.0),
        }
    }
}

/// 一个能看的 Run：能读它的会话。看不到的与不存在一样回 404。
pub(super) async fn readable_run(state: &AppState, ctx: &IamContext, id: &str) -> Result<Run> {
    let run = state
        .agent
        .runs
        .get(&ctx.org_id, &Id::from_string(id))
        .await?;

    match readable_chat(state, ctx, &run.chat_id).await {
        Ok(_) => Ok(run),
        Err(Error::Forbidden(_) | Error::NotFound(_)) => {
            Err(Error::not_found(format!("agent run `{id}` not found")))
        }
        Err(error) => Err(error),
    }
}

#[permission("agent.use")]
async fn get_run(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<RunResp>> {
    Ok(Json(RunResp::of(&readable_run(&state, &ctx, &id).await?)))
}

#[derive(Debug, Deserialize)]
struct ChatRunsQuery {
    #[serde(default)]
    limit: Option<i64>,
}

#[permission("agent.use")]
async fn chat_runs(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(chat_id): Path<String>,
    Query(query): Query<ChatRunsQuery>,
) -> Result<Json<Value>> {
    let (chat, _) = readable_chat(&state, &ctx, &Id(chat_id)).await?;

    let runs = state
        .agent
        .runs
        .for_chat(&ctx.org_id, &chat.id, query.limit.unwrap_or(20))
        .await?;

    Ok(Json(json!({
        "runs": runs.iter().map(RunResp::of).collect::<Vec<_>>(),
    })))
}

#[permission("agent.use")]
async fn cancel_run(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<RunResp>> {
    let run = readable_run(&state, &ctx, &id).await?;
    let run = state
        .agent
        .runs
        .request_cancel(&ctx.org_id, &run.id)
        .await?;

    activity_audit::record(
        &state,
        &ctx,
        "agent.run.cancelled",
        "agent_chat",
        &run.chat_id.0,
        json!({ "runId": run.id.0, "status": run.status.as_str() }),
    )
    .await;

    Ok(Json(RunResp::of(&run)))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RespondReq {
    call_id: String,
    /// 选中的选项。
    #[serde(default)]
    choices: Vec<String>,
    /// 写下的回答。
    #[serde(default)]
    text: Option<String>,
    /// 确认：同意或拒绝。
    #[serde(default)]
    approved: Option<bool>,
    /// 授权：存好了凭据接着执行，或跳过这台服务器。
    #[serde(default)]
    authorized: Option<bool>,
}

/// 把请求读成对在等的东西的回答；对不上时报错。
fn response_to(
    pending: &Pending,
    req: RespondReq,
    ctx: &IamContext,
    initiator: &Id,
) -> Result<RunResponse> {
    match pending {
        Pending::Confirmation { .. } => {
            let approved = req
                .approved
                .ok_or_else(|| Error::invalid("a confirmation needs `approved`"))?;

            // 工具以发起人的身份执行，只有他能决定。
            if ctx.user_id != *initiator {
                return Err(Error::forbidden(
                    "only the person who asked can confirm a tool in their answer",
                ));
            }

            Ok(RunResponse::Decision {
                call_id: req.call_id,
                approved,
                responder: ctx.user_id.clone(),
            })
        }
        Pending::Question { kind, options, .. } => {
            let text = req.text.unwrap_or_default().trim().to_string();
            let choices = req.choices;

            if choices.iter().any(|choice| !options.contains(choice)) {
                return Err(Error::invalid(
                    "every choice must be one of the question's options",
                ));
            }

            let enough = match kind {
                QuestionKind::Single => {
                    choices.len() == 1 || (choices.is_empty() && !text.is_empty())
                }
                QuestionKind::Multiple => !choices.is_empty() || !text.is_empty(),
                QuestionKind::Text => !text.is_empty(),
            };

            if !enough {
                return Err(Error::invalid("the answer is empty"));
            }

            Ok(RunResponse::Answer {
                call_id: req.call_id,
                choices,
                text,
                responder: ctx.user_id.clone(),
            })
        }
        Pending::Authorization { .. } => {
            let authorized = req
                .authorized
                .ok_or_else(|| Error::invalid("an authorization needs `authorized`"))?;

            // 凭据是发起人自己的，工具也以他的身份执行。
            if ctx.user_id != *initiator {
                return Err(Error::forbidden(
                    "only the person who asked can authorize a tool in their answer",
                ));
            }

            Ok(RunResponse::Authorization {
                call_id: req.call_id,
                authorized,
                responder: ctx.user_id.clone(),
            })
        }
    }
}

#[permission("agent.use")]
async fn respond(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(req): Json<RespondReq>,
) -> Result<Json<RunResp>> {
    let run = readable_run(&state, &ctx, &id).await?;

    let pending = run
        .pending
        .as_ref()
        .filter(|pending| pending.call_id() == req.call_id)
        .ok_or_else(|| {
            Error::conflict(
                "the answer is not waiting for this question, confirmation or authorization",
            )
        })?;

    let response = response_to(pending, req, &ctx, run.initiator())?;

    // 说存好了就要真的存过：不然再调一次还是缺凭据。
    if let (
        Pending::Authorization { server_id, .. },
        RunResponse::Authorization {
            authorized: true, ..
        },
    ) = (pending, &response)
        && !state
            .agent
            .mcp
            .has_my_credential(&ctx.org_id, &ctx.user_id, &Id(server_id.clone()))
            .await?
    {
        return Err(Error::conflict(
            "save your credential for this MCP server before continuing",
        ));
    }

    let run = state
        .agent
        .runs
        .respond(&ctx.org_id, &run.id, &response)
        .await?;

    let detail = match &response {
        RunResponse::Decision { approved, .. } => {
            json!({ "runId": run.id.0, "approved": approved })
        }
        RunResponse::Answer { .. } => json!({ "runId": run.id.0, "answered": true }),
        RunResponse::Authorization { authorized, .. } => {
            json!({ "runId": run.id.0, "authorized": authorized })
        }
    };

    activity_audit::record(
        &state,
        &ctx,
        "agent.run.responded",
        "agent_chat",
        &run.chat_id.0,
        detail,
    )
    .await;

    Ok(Json(RunResp::of(&run)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{agent::model::RiskLevel, domain::iam::IamScope};

    fn ctx(user: &str) -> IamContext {
        crate::agent::runs::RunPrincipal {
            user_id: Id::from_string(user),
            scope: IamScope::Organization,
            credential_token_id: None,
            credential_application_id: None,
            credential_service_account_id: None,
        }
        .unresolved_context(&Id::from_string("org-1"))
    }

    fn request(value: Value) -> RespondReq {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn only_the_person_who_asked_decides_a_confirmation() {
        let pending = Pending::Confirmation {
            call_id: "c1".into(),
            tool: "restart_service".into(),
            arguments: json!({}),
            risk: RiskLevel::L1,
        };

        let asker = Id::from_string("u1");
        let ok = response_to(
            &pending,
            request(json!({"callId": "c1", "approved": true})),
            &ctx("u1"),
            &asker,
        );
        assert!(matches!(
            ok,
            Ok(RunResponse::Decision { approved: true, .. })
        ));

        let other = response_to(
            &pending,
            request(json!({"callId": "c1", "approved": true})),
            &ctx("u2"),
            &asker,
        );
        assert!(matches!(other, Err(Error::Forbidden(_))));

        let missing = response_to(
            &pending,
            request(json!({"callId": "c1"})),
            &ctx("u1"),
            &asker,
        );
        assert!(missing.is_err());
    }

    #[test]
    fn only_the_person_who_asked_authorizes_with_their_own_credential() {
        let pending = Pending::Authorization {
            call_id: "a1".into(),
            tool: "linear_search_issues".into(),
            server_id: "server-1".into(),
            server: "Linear".into(),
            header: "authorization".into(),
            bearer: true,
            instructions: String::new(),
            rejected: false,
        };

        let asker = Id::from_string("u1");

        for authorized in [true, false] {
            let ok = response_to(
                &pending,
                request(json!({"callId": "a1", "authorized": authorized})),
                &ctx("u1"),
                &asker,
            );
            assert!(
                matches!(ok, Ok(RunResponse::Authorization { authorized: value, .. }) if value == authorized)
            );
        }

        let teammate = response_to(
            &pending,
            request(json!({"callId": "a1", "authorized": true})),
            &ctx("u2"),
            &asker,
        );
        assert!(matches!(teammate, Err(Error::Forbidden(_))));

        // 确认的 `approved` 不能当作授权。
        let wrong = response_to(
            &pending,
            request(json!({"callId": "a1", "approved": true})),
            &ctx("u1"),
            &asker,
        );
        assert!(wrong.is_err());
    }

    #[test]
    fn an_answer_must_pick_from_the_options_or_say_something() {
        let pending = Pending::Question {
            call_id: "q1".into(),
            question: "Which environment?".into(),
            kind: QuestionKind::Single,
            options: vec!["staging".into(), "production".into()],
        };

        let asker = Id::from_string("u1");
        let teammate = ctx("u2");

        assert!(
            response_to(
                &pending,
                request(json!({"callId": "q1", "choices": ["production"]})),
                &teammate,
                &asker
            )
            .is_ok()
        );
        assert!(
            response_to(
                &pending,
                request(json!({"callId": "q1", "choices": ["dev"]})),
                &teammate,
                &asker
            )
            .is_err()
        );
        assert!(
            response_to(
                &pending,
                request(json!({"callId": "q1", "choices": ["staging", "production"]})),
                &teammate,
                &asker
            )
            .is_err()
        );
        assert!(
            response_to(
                &pending,
                request(json!({"callId": "q1", "text": "the canary"})),
                &teammate,
                &asker
            )
            .is_ok()
        );
        assert!(
            response_to(
                &pending,
                request(json!({"callId": "q1"})),
                &teammate,
                &asker
            )
            .is_err()
        );
    }
}
