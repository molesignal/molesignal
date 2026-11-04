// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Mole Agent 的质量：排上与执行打分、待审、Prompt 晋级检查，以及「质量」页签的接口。
//!
//! - 读：[`views`]（概览、打过分的回答、一个回答的分数与材料）
//! - 待审：[`reviews`]；评审与设置：[`evaluators`]；晋级检查：[`checks`]
//! - 执行：[`process`]（规则与评审），事实来自 [`facts`]，评审调模型在 [`evaluating`]，检查里
//!   的重新回答在 [`replay`]
//!
//! 接口都要 `agent.manage`：持有它的人本来就能读团队对话与组织的自动化。个人对话只在合计里。
//! 规则见 `agent::scoring`。

mod checks;
mod evaluating;
mod evaluators;
mod facts;
mod process;
mod replay;
mod reviews;
mod views;

use axum::Router;
pub(crate) use checks::{gate_set_default, process_next as process_next_check};
pub(crate) use process::process_due;

use crate::{
    agent::{
        feedback::FeedbackTarget,
        runs::Run,
        scoring::{ScoreTarget, TargetSource},
    },
    api::AppState,
    app::scoring::{enqueue_answered, enqueue_rated},
    shared::{Result, ids::Id},
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .merge(views::routes())
        .merge(reviews::routes())
        .merge(evaluators::routes())
        .merge(checks::routes())
}

/// 一轮回答结束、写下了回答之后排上打分。出错只记日志：打分不挡回答。
pub(crate) async fn answered(state: &AppState, run: &Run, message_id: &Id) {
    let queued = async {
        let chat = state
            .agent
            .chats
            .get_chat(&run.org_id, &run.chat_id)
            .await?;

        enqueue_answered(
            state.agent.scoring.as_ref(),
            &run.org_id,
            (ScoreTarget::ChatAnswer, message_id.0.as_str()),
            Some(&chat.id),
            facts::chat_source(&chat),
        )
        .await
    };

    if let Err(error) = queued.await {
        tracing::warn!(run_id = %run.id.0, %error, "an answer could not be queued for scoring");
    }
}

/// 一条评价提交或撤回之后排上打分：规则里的评价要重新算，评为没帮助时评审必评。
pub(crate) async fn rated(
    state: &AppState,
    org_id: &Id,
    target: (FeedbackTarget, &str),
    chat_id: Option<&Id>,
    down: bool,
) {
    let queued = async {
        let (kind, chat_id, source) = match (target.0, chat_id) {
            (FeedbackTarget::ChatMessage, Some(chat_id)) => {
                let chat = state.agent.chats.get_chat(org_id, chat_id).await?;
                let source = facts::chat_source(&chat);

                (ScoreTarget::ChatAnswer, Some(chat.id), source)
            }
            (FeedbackTarget::ChatMessage, None) => return Result::Ok(()),
            (FeedbackTarget::IncidentRca, _) => {
                (ScoreTarget::IncidentRca, None, TargetSource::IncidentRca)
            }
        };

        enqueue_rated(
            state.agent.scoring.as_ref(),
            org_id,
            (kind, target.1),
            chat_id.as_ref(),
            source,
            down,
        )
        .await
    };

    if let Err(error) = queued.await {
        tracing::warn!(target = target.1, %error, "a rating could not be queued for scoring");
    }
}
