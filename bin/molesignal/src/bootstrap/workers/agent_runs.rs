// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Mole Agent 的 Run：领取与执行、把没了节点的 Run 标为中断、清理旧事件。
//!
//! 提供完整 HTTP 的节点（standalone、router）各起一份：
//! 1. 领取循环：有执行名额时领走最早能执行的 Run（同一会话一个接一个），交给执行器。排队与结束
//!    时经 NOTIFY 被叫醒，退避轮询只是兜底。名额数是 `[agent] max_concurrent_runs`；等提问人
//!    回答的 Run 让出名额。
//! 2. 看门：租约过期的 Run（执行它的节点没了）标为中断，补一个结束事件。
//! 3. 清理：七天前的事件删掉；回答本身在 `agent_messages`。
//! 4. 提炼记忆：组织打开了「会话结束后提炼」时，停下半小时的会话排一个提炼 Run，以会话发起人
//!    的身份执行。

use std::{sync::Arc, time::Duration};

use tokio::{sync::Semaphore, task::JoinHandle};
use tracing::Instrument;

use super::polling::PollingBackoff;
use crate::{
    agent::runs::{Run, RunPrincipal, RunRepository, RunSource},
    api::{
        AppState,
        http::routes::agent::chat::run::{RunSlot, execute, lease_until, request::RunRequest},
    },
    config::Settings,
    domain::iam::IamScope,
    shared::time::TimestampMicros,
};

/// 看门多久一次。
const REAP_EVERY: Duration = Duration::from_secs(15);
/// 清理多久一次，事件留多久。
const PURGE_EVERY: Duration = Duration::from_secs(60 * 60);
const EVENT_RETENTION: Duration = Duration::from_secs(7 * 24 * 60 * 60);
/// 多久找一次该提炼记忆的会话；会话停下多久算结束；一次最多排几个。
const DISTILL_EVERY: Duration = Duration::from_secs(5 * 60);
const DISTILL_IDLE: Duration = Duration::from_secs(30 * 60);
const DISTILL_BATCH: i64 = 20;

pub struct AgentRunWorker {
    state: AppState,
    node: String,
    max_concurrent: usize,
    context_window: u64,
}

impl AgentRunWorker {
    pub fn new(state: AppState, settings: &Settings) -> Self {
        Self {
            node: state.cluster.node_id.clone(),
            state,
            max_concurrent: settings.agent.max_concurrent_runs.max(1),
            context_window: settings.agent.max_context_tokens,
        }
    }

    pub fn spawn(self) -> Vec<JoinHandle<()>> {
        let worker = Arc::new(self);

        vec![
            tokio::spawn(worker.clone().claim_loop()),
            tokio::spawn(worker.reap_loop()),
        ]
    }

    async fn claim_loop(self: Arc<Self>) {
        let runs = self.state.agent.runs.clone();
        let semaphore = Arc::new(Semaphore::new(self.max_concurrent));

        let mut backoff = PollingBackoff::new(
            Duration::from_secs(2),
            Duration::from_secs(30),
            "agent-runs-poller",
        );

        tracing::info!(
            max_concurrent = self.max_concurrent,
            "agent run worker started"
        );

        loop {
            let Ok(permit) = semaphore.clone().acquire_owned().await else {
                return;
            };

            match runs.claim_next(&self.node, lease_until()).await {
                Ok(Some(run)) => {
                    backoff.reset();

                    let span = tracing::info_span!(
                        parent: None,
                        "worker.agent_run",
                        otel.kind = "internal",
                        molesignal.worker.name = "agent_runs",
                        run_id = %run.id.0,
                        chat_id = %run.chat_id.0,
                    );

                    let slot = RunSlot::new(semaphore.clone(), permit);

                    tokio::spawn(
                        execute(
                            self.state.clone(),
                            run,
                            self.node.clone(),
                            slot,
                            self.context_window,
                        )
                        .instrument(span),
                    );

                    continue;
                }
                Ok(None) => drop(permit),
                Err(error) => {
                    tracing::warn!(%error, "an agent run could not be claimed");
                    drop(permit);
                }
            }

            runs.wait_for_pending(backoff.next_delay()).await;
        }
    }

    async fn reap_loop(self: Arc<Self>) {
        let runs = self.state.agent.runs.clone();
        let mut ticker = tokio::time::interval(REAP_EVERY);
        let mut purged_at: Option<tokio::time::Instant> = None;
        let mut distilled_at: Option<tokio::time::Instant> = None;

        loop {
            ticker.tick().await;
            self.reap(&runs).await;

            if purged_at.is_none_or(|at| at.elapsed() >= PURGE_EVERY) {
                purged_at = Some(tokio::time::Instant::now());
                self.purge(&runs).await;
            }

            if distilled_at.is_none_or(|at| at.elapsed() >= DISTILL_EVERY) {
                distilled_at = Some(tokio::time::Instant::now());
                self.queue_distillation(&runs).await;
            }
        }
    }

    #[tracing::instrument(
        name = "worker.agent_memory_distillation",
        parent = None,
        skip_all,
        fields(otel.kind = "internal", molesignal.worker.name = "agent_memory_distillation")
    )]
    async fn queue_distillation(&self, runs: &Arc<dyn RunRepository>) {
        let idle_before =
            TimestampMicros(TimestampMicros::now().0 - DISTILL_IDLE.as_micros() as i64);

        let due = match self
            .state
            .agent
            .chats
            .due_for_distillation(idle_before, DISTILL_BATCH)
            .await
        {
            Ok(due) => due,
            Err(error) => {
                tracing::warn!(%error, "chats due for memory distillation could not be read");
                return;
            }
        };

        for (chat, since_micros) in due {
            let request =
                serde_json::to_value(RunRequest::Distill { since_micros }).unwrap_or_default();

            let principal = RunPrincipal {
                user_id: chat.user_id.clone(),
                scope: IamScope::Organization,
                credential_role_id: None,
                credential_application_id: None,
                credential_service_account_id: None,
            };

            let run = Run::queued(
                chat.org_id,
                chat.id,
                RunSource::Chat,
                principal,
                request,
                None,
            );

            if let Err(error) = runs.create(run).await {
                tracing::warn!(%error, "a memory distillation could not be queued");
            }
        }
    }

    #[tracing::instrument(
        name = "worker.agent_run_reaper",
        parent = None,
        skip_all,
        fields(otel.kind = "internal", molesignal.worker.name = "agent_run_reaper")
    )]
    async fn reap(&self, runs: &Arc<dyn RunRepository>) {
        match runs.expire_leases(TimestampMicros::now()).await {
            Ok(expired) if !expired.is_empty() => {
                tracing::warn!(
                    count = expired.len(),
                    "agent runs interrupted after their node stopped"
                );
            }
            Ok(_) => {}
            Err(error) => tracing::warn!(%error, "expired agent runs could not be interrupted"),
        }
    }

    #[tracing::instrument(
        name = "worker.agent_run_events_purge",
        parent = None,
        skip_all,
        fields(otel.kind = "internal", molesignal.worker.name = "agent_run_events_purge")
    )]
    async fn purge(&self, runs: &Arc<dyn RunRepository>) {
        let before = TimestampMicros(TimestampMicros::now().0 - EVENT_RETENTION.as_micros() as i64);

        match runs.purge_events(before).await {
            Ok(0) => {}
            Ok(purged) => tracing::info!(purged, "old agent run events purged"),
            Err(error) => tracing::warn!(%error, "old agent run events could not be purged"),
        }
    }
}
