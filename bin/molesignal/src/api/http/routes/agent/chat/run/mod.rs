// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 在后台执行一个 Run。
//!
//! 领取 Run 的节点（`bootstrap::workers::agent_runs`）调 [`execute`]：按 Run 记下的身份重新核对
//! 权限，读出会话与请求，回答或压缩，过程写成事件，最后记下结果。执行期间有一个看守：续租约，
//! 留意停止请求；租约续不上（Run 已被标为中断）也当作停下。
//!
//! - [`answer`]：一轮回答。
//! - [`history`]：交给模型的历史与上下文压缩。
//! - [`sink`]：把 AgentLoop 的事件写成 Run 的事件，记下工具调用与证据。
//! - [`interaction`]：任务清单、提问与确认。
//! - [`workspace`]：会话自动带上的知识入口文件与记忆目录。
//! - [`title`]：标题还是临时的时候给对话起名。

use std::{sync::Arc, time::Duration};

use parking_lot::Mutex;
use serde_json::{Value, json};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio_util::sync::CancellationToken;

use crate::{
    agent::{
        runs::{NewRunEvent, Run, RunEventKind, RunOutcome, RunStatus},
        usage::Cost,
    },
    api::AppState,
    shared::{Error, ids::Id, time::TimestampMicros},
};

mod answer;
mod distill;
mod history;
mod identity;
mod interaction;
pub(crate) mod request;
mod sink;
mod title;
mod workspace;

use request::RunRequest;

/// 租约多长；看守每隔 [`RENEW_EVERY`] 续一次，节点没了之后最多这么久 Run 被标为中断。
pub(crate) const LEASE: Duration = Duration::from_secs(45);
const RENEW_EVERY: Duration = Duration::from_secs(10);
/// 没有提醒时多久自己查一次停止请求。
const CANCEL_POLL: Duration = Duration::from_secs(2);

/// 现在起一个租约期之后。
pub(crate) fn lease_until() -> TimestampMicros {
    TimestampMicros(TimestampMicros::now().0 + LEASE.as_micros() as i64)
}

/// 一个执行名额。等提问人回答时让出，回来再拿。
pub(crate) struct RunSlot {
    semaphore: Arc<Semaphore>,
    permit: Mutex<Option<OwnedSemaphorePermit>>,
}

impl RunSlot {
    pub(crate) fn new(semaphore: Arc<Semaphore>, permit: OwnedSemaphorePermit) -> Self {
        Self {
            semaphore,
            permit: Mutex::new(Some(permit)),
        }
    }

    fn release(&self) {
        self.permit.lock().take();
    }

    /// 拿回名额；被停下时不再等。
    async fn reacquire(&self, cancel: &CancellationToken) {
        if self.permit.lock().is_some() {
            return;
        }

        let permit = tokio::select! {
            _ = cancel.cancelled() => None,
            permit = self.semaphore.clone().acquire_owned() => permit.ok(),
        };

        *self.permit.lock() = permit;
    }
}

/// 执行中的一个 Run。
pub(super) struct RunControl {
    pub(super) state: AppState,
    pub(super) run: Run,
    node: String,
    pub(super) cancel: CancellationToken,
    slot: RunSlot,
    /// 实例设置的上下文大小；Agent Profile 设了时用 Profile 的。
    pub(super) context_window: u64,
}

impl RunControl {
    pub(super) async fn emit(&self, kind: RunEventKind, payload: Value) {
        self.emit_all(vec![NewRunEvent::new(kind, payload)]).await;
    }

    /// 写事件。Run 已经结束（例如已被标为中断）时当作被停下。
    pub(super) async fn emit_all(&self, events: Vec<NewRunEvent>) {
        let written = self
            .state
            .agent
            .runs
            .append_events(&self.run.org_id, &self.run.id, &events)
            .await;

        match written {
            Ok(_) => {}
            Err(Error::Conflict(_)) => self.cancel.cancel(),
            Err(error) => {
                tracing::warn!(run_id = %self.run.id.0, %error, "an agent run event could not be written");
            }
        }
    }
}

/// 一轮怎么结束的。
#[derive(Debug)]
pub(super) struct Ending {
    pub(super) status: RunStatus,
    pub(super) message_id: Option<Id>,
    /// 成功时的回答。
    pub(super) content: Option<String>,
    pub(super) prompt_tokens: i64,
    pub(super) completion_tokens: i64,
    pub(super) cost: Option<Cost>,
    pub(super) finish_reason: Option<String>,
    pub(super) error: Option<String>,
    /// 前端据此写说明的固定代码，例如预算用完。
    pub(super) code: Option<&'static str>,
}

impl Ending {
    fn of(status: RunStatus) -> Self {
        Self {
            status,
            message_id: None,
            content: None,
            prompt_tokens: 0,
            completion_tokens: 0,
            cost: None,
            finish_reason: None,
            error: None,
            code: None,
        }
    }

    pub(super) fn succeeded() -> Self {
        Self::of(RunStatus::Succeeded)
    }

    pub(super) fn failed(error: impl Into<String>) -> Self {
        Self {
            error: Some(error.into()),
            ..Self::of(RunStatus::Failed)
        }
    }

    /// 没有开始就被拒绝，`code` 说明原因。
    pub(super) fn refused(code: &'static str) -> Self {
        Self {
            code: Some(code),
            ..Self::failed(code)
        }
    }
}

/// 执行一个已领走的 Run，结束时记下结果。`context_window` 是实例设置的上下文大小。
pub(crate) async fn execute(
    state: AppState,
    run: Run,
    node: String,
    slot: RunSlot,
    context_window: u64,
) {
    let control = Arc::new(RunControl {
        state,
        run,
        node,
        cancel: CancellationToken::new(),
        slot,
        context_window,
    });

    let watching = CancellationToken::new();
    let watcher = tokio::spawn(watch(control.clone(), watching.clone()));

    control
        .emit(RunEventKind::Status, json!({ "status": "running" }))
        .await;

    let mut ending = perform(&control).await;

    // 停下与失败碰在一起时（例如停止打断了模型调用）按停下算。
    if control.cancel.is_cancelled() && ending.status == RunStatus::Failed && ending.code.is_none()
    {
        ending.status = RunStatus::Cancelled;
        ending.error = None;
    }

    finish(&control, ending).await;
    watching.cancel();
    let _ = watcher.await;
}

async fn perform(control: &Arc<RunControl>) -> Ending {
    let state = &control.state;
    let run = &control.run;

    let ctx = match identity::resolve(state, run).await {
        Ok(ctx) => ctx,
        Err(error) => return Ending::failed(error.to_string()),
    };

    let chat = match state.agent.chats.get_chat(&run.org_id, &run.chat_id).await {
        Ok(chat) => chat,
        Err(Error::NotFound(_)) => return Ending::failed("the chat was deleted"),
        Err(error) => return Ending::failed(error.to_string()),
    };

    let request = match serde_json::from_value::<RunRequest>(run.request.clone()) {
        Ok(request) => request,
        Err(error) => {
            return Ending::failed(format!("the queued request could not be read: {error}"));
        }
    };

    match request {
        RunRequest::Answer(request) => answer::answer(control, &ctx, chat, *request).await,
        RunRequest::Compact => history::compact_now(control, &ctx, chat).await,
        RunRequest::Distill { since_micros } => {
            distill::distill(control, &ctx, chat, since_micros).await
        }
    }
}

/// 续租约，留意停止请求，直到 `done`。
async fn watch(control: Arc<RunControl>, done: CancellationToken) {
    let runs = &control.state.agent.runs;
    let run_id = &control.run.id;
    let mut signals = runs.signals();
    let mut renew_at = tokio::time::Instant::now() + RENEW_EVERY;

    loop {
        tokio::select! {
            _ = done.cancelled() => return,
            _ = signals.changed(run_id, CANCEL_POLL) => {}
        }

        if !control.cancel.is_cancelled() && runs.cancel_requested(run_id).await.unwrap_or(false) {
            control.cancel.cancel();
        }

        if tokio::time::Instant::now() < renew_at {
            continue;
        }

        renew_at = tokio::time::Instant::now() + RENEW_EVERY;

        match runs.renew(run_id, &control.node, lease_until()).await {
            Ok(true) => {}
            // 已经不归这个节点：被标为中断了，停下。
            Ok(false) => control.cancel.cancel(),
            Err(error) => {
                tracing::warn!(run_id = %run_id.0, %error, "an agent run lease could not be renewed")
            }
        }
    }
}

async fn finish(control: &RunControl, ending: Ending) {
    let outcome = RunOutcome {
        status: ending.status,
        assistant_message_id: ending.message_id.clone(),
        prompt_tokens: ending.prompt_tokens,
        completion_tokens: ending.completion_tokens,
        cost: ending.cost,
        error: ending.error.clone(),
    };

    let done = NewRunEvent::new(
        RunEventKind::Done,
        json!({
            "status": ending.status.as_str(),
            "message_id": ending.message_id.map(|id| id.0),
            "content": ending.content,
            "prompt_tokens": ending.prompt_tokens,
            "completion_tokens": ending.completion_tokens,
            "cost": ending.cost,
            "finish_reason": ending.finish_reason,
            "error": ending.error,
            "code": ending.code,
        }),
    );

    match control
        .state
        .agent
        .runs
        .finish(&control.run, &outcome, &[done])
        .await
    {
        Ok(true) => {}
        Ok(false) => tracing::debug!(run_id = %control.run.id.0, "the agent run was already over"),
        Err(error) => {
            tracing::warn!(run_id = %control.run.id.0, %error, "an agent run could not be finished")
        }
    }
}
