// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Mole Agent 在 IM 里的长连接：每个开着的机器人由一个节点连着（数据库里的租约），收到的消息交给
//! `agent::im` 处理。结束了还没回复的在这里补上，旧的消息记录定期清掉。
//!
//! 提供完整 HTTP 的节点各起一份：每轮列出开着的机器人，没人连着的领下来连上，自己连着的续租；
//! 机器人关了、删了、连接器改了或租约丢了就断开。连接断了从 5 秒起翻倍、最多 5 分钟后重连，
//! 原因写进机器人的状态。回复只能走长连接的机器人（企业微信），连着的节点每 2 秒发一次别的节点
//! 交来的回复。告警卡片上按钮的点击也从这里交给 `agent::im` 处理。

use std::{collections::HashMap, sync::Arc, time::Duration};

use async_trait::async_trait;
use tokio::{sync::oneshot, task::JoinHandle};
use tokio_util::sync::CancellationToken;

use crate::{
    agent::im::{BotStatus, CardAction, CardAnswer, ImBot, ImPlatform, Inbound},
    api::{
        AppState,
        http::routes::agent::im::{act, drain, handle, reconcile},
    },
    domain::notify::connector::NotifyConnector,
    infra::im::{self, Listener},
    shared::{ids::Id, time::TimestampMicros},
};

/// 多久看一次机器人、续一次租。
const TICK: Duration = Duration::from_secs(10);
/// 租约多久：节点没了，别的节点这么久之后接手。
const LEASE_MICROS: i64 = 60 * 1_000_000;
/// 多久补一次没回复的。
const RECONCILE_EVERY: Duration = Duration::from_secs(30);
/// 多久清一次旧消息，留多久。
const PURGE_EVERY: Duration = Duration::from_secs(60 * 60);
const KEEP_MICROS: i64 = 30 * 24 * 60 * 60 * 1_000_000;
const BACKOFF_START: Duration = Duration::from_secs(5);
const BACKOFF_MAX: Duration = Duration::from_secs(300);
/// 连着的节点多久发一次别的节点交来的回复。
const DRAIN_EVERY: Duration = Duration::from_secs(2);

/// 一个连着的机器人：设置或连接器变了就要重连。
struct Connection {
    version: (TimestampMicros, TimestampMicros),
    cancel: CancellationToken,
    task: JoinHandle<()>,
}

pub struct ImWorker {
    state: AppState,
    node: String,
}

impl ImWorker {
    pub fn new(state: AppState) -> Self {
        let node = state.cluster.node_id.clone();
        Self { state, node }
    }

    pub fn spawn(self) -> JoinHandle<()> {
        let worker = Arc::new(self);
        let upkeep = worker.clone();

        tokio::spawn(async move {
            let mut purged_at = tokio::time::Instant::now();

            loop {
                tokio::time::sleep(RECONCILE_EVERY).await;

                let purge = purged_at.elapsed() >= PURGE_EVERY;

                if purge {
                    purged_at = tokio::time::Instant::now();
                }

                upkeep.upkeep(purge).await;
            }
        });

        tokio::spawn(async move {
            let mut connections: HashMap<Id, Connection> = HashMap::new();

            loop {
                worker.tick(&mut connections).await;
                tokio::time::sleep(TICK).await;
            }
        })
    }

    #[tracing::instrument(
        name = "worker.agent_im",
        parent = None,
        skip_all,
        fields(otel.kind = "internal", molesignal.worker.name = "agent_im")
    )]
    async fn tick(&self, connections: &mut HashMap<Id, Connection>) {
        let repository = &self.state.agent.im;

        let bots = match repository.enabled_bots().await {
            Ok(bots) => bots,
            Err(error) => {
                tracing::warn!(%error, "the IM bots could not be listed");
                return;
            }
        };

        // 关了或删了的：断开，交还租约。
        let gone = connections
            .keys()
            .filter(|id| !bots.iter().any(|bot| &bot.connector_id == *id))
            .cloned()
            .collect::<Vec<_>>();

        for id in gone {
            if let Some(connection) = connections.remove(&id) {
                connection.cancel.cancel();
            }

            let _ = repository.release_bot(&id, &self.node).await;
        }

        for bot in bots {
            let now = TimestampMicros::now();
            let leased = repository
                .lease_bot(
                    &bot.connector_id,
                    &self.node,
                    now,
                    TimestampMicros(now.0 + LEASE_MICROS),
                )
                .await
                .unwrap_or(false);

            if !leased {
                if let Some(connection) = connections.remove(&bot.connector_id) {
                    connection.cancel.cancel();
                }

                continue;
            }

            let connector = match self
                .state
                .alerting
                .notify
                .get_connector(&bot.org_id, &bot.connector_id)
                .await
            {
                Ok(connector) => connector,
                Err(error) => {
                    tracing::warn!(connector = %bot.connector_id.0, %error, "an IM connector could not be read");
                    continue;
                }
            };

            let version = (bot.updated_at, connector.updated_at);

            match connections.get(&bot.connector_id) {
                Some(connection)
                    if connection.version == version && !connection.task.is_finished() =>
                {
                    continue;
                }
                Some(connection) => connection.cancel.cancel(),
                None => {}
            }

            connections.remove(&bot.connector_id);

            if !connector.enabled {
                let _ = repository
                    .set_bot_status(
                        &bot.connector_id,
                        &self.node,
                        BotStatus::Error,
                        Some("the connector is switched off"),
                    )
                    .await;
                continue;
            }

            let cancel = CancellationToken::new();
            let task = tokio::spawn(run(
                self.state.clone(),
                bot.clone(),
                connector,
                cancel.clone(),
            ));

            connections.insert(
                bot.connector_id.clone(),
                Connection {
                    version,
                    cancel,
                    task,
                },
            );
        }
    }

    async fn upkeep(&self, purge: bool) {
        if let Err(error) = reconcile(&self.state).await {
            tracing::warn!(%error, "unanswered IM messages could not be settled");
        }

        if purge {
            let before = TimestampMicros(TimestampMicros::now().0 - KEEP_MICROS);

            if let Err(error) = self.state.agent.im.purge_messages(before).await {
                tracing::warn!(%error, "old IM messages could not be purged");
            }
        }
    }
}

/// 长连接交出来的东西：连上了写状态，收到的消息另起任务处理（平台在等确认）。
struct Relay {
    state: AppState,
    bot: ImBot,
    connector: NotifyConnector,
    node: String,
}

#[async_trait]
impl Listener for Relay {
    async fn connected(&self, detail: &str) {
        let detail = format!("{detail} · {}", self.node);
        let _ = self
            .state
            .agent
            .im
            .set_bot_status(
                &self.bot.connector_id,
                &self.node,
                BotStatus::Connected,
                Some(&detail),
            )
            .await;
    }

    async fn message(&self, inbound: Inbound) {
        let (state, bot, connector) =
            (self.state.clone(), self.bot.clone(), self.connector.clone());

        tokio::spawn(async move { handle(&state, &bot, &connector, inbound).await });
    }

    /// 另起任务处理，平台在确认里等结果时（飞书）最多等一会儿；来不及的由任务自己换上。
    async fn action(&self, action: CardAction) -> Option<CardAnswer> {
        let platform = ImPlatform::of_connector(&self.connector.connector_type)?;
        let waiting = im::answers_in_acknowledgement(platform);
        let (state, bot, connector) =
            (self.state.clone(), self.bot.clone(), self.connector.clone());
        let (sender, mut receiver) = oneshot::channel::<CardAnswer>();

        tokio::spawn(async move {
            let answer = act(&state, &bot, &connector, &action).await;

            // 长连接还在等就交给它，不然自己换上。
            let answer = if waiting {
                match sender.send(answer) {
                    Ok(()) => return,
                    Err(answer) => answer,
                }
            } else {
                answer
            };

            if let Err(error) = im::answer(platform, &connector.config, &action, &answer).await {
                tracing::warn!(connector = %bot.connector_id.0, %error, "an alert card could not be updated");
            }
        });

        if !waiting {
            return None;
        }

        match tokio::time::timeout(im::ANSWER_WITHIN, &mut receiver).await {
            Ok(answer) => answer.ok(),
            // 不再等了：之前送到的还收得到，之后的由任务自己换上。
            Err(_) => {
                receiver.close();
                receiver.try_recv().ok()
            }
        }
    }
}

/// 连着一个机器人，断了就重连，直到 `cancel`。
async fn run(state: AppState, bot: ImBot, connector: NotifyConnector, cancel: CancellationToken) {
    let Some(platform) = ImPlatform::of_connector(&connector.connector_type) else {
        return;
    };

    let bots = state.agent.im.clone();
    let node = state.cluster.node_id.clone();

    if im::replies_over_connection(platform) {
        tokio::spawn(drain_while(
            state.clone(),
            connector.clone(),
            cancel.clone(),
        ));
    }

    let relay = Relay {
        node: node.clone(),
        state,
        bot: bot.clone(),
        connector: connector.clone(),
    };
    let mut backoff = BACKOFF_START;

    while !cancel.is_cancelled() {
        let _ = bots
            .set_bot_status(&bot.connector_id, &node, BotStatus::Connecting, None)
            .await;

        let started = tokio::time::Instant::now();
        let result = im::listen(platform, &connector.config, &relay, &cancel).await;

        if cancel.is_cancelled() {
            break;
        }

        let wait = match result {
            // 平台要求重连：马上连。
            Ok(()) => Duration::from_secs(1),
            Err(error) => {
                let detail = error.to_string();
                tracing::warn!(connector = %bot.connector_id.0, error = %detail, "an IM connection failed");
                let _ = bots
                    .set_bot_status(&bot.connector_id, &node, BotStatus::Error, Some(&detail))
                    .await;

                // 连上过一阵子再断的，从头退避。
                if started.elapsed() > BACKOFF_MAX {
                    backoff = BACKOFF_START;
                }

                let wait = backoff;
                backoff = (backoff * 2).min(BACKOFF_MAX);
                wait
            }
        };

        tokio::select! {
            () = cancel.cancelled() => break,
            () = tokio::time::sleep(wait) => {}
        }
    }
}

/// 连着一个机器人期间，发别的节点交来的回复，直到 `cancel`。
async fn drain_while(state: AppState, connector: NotifyConnector, cancel: CancellationToken) {
    loop {
        tokio::select! {
            () = cancel.cancelled() => return,
            () = tokio::time::sleep(DRAIN_EVERY) => {}
        }

        if let Err(error) = drain(&state, &connector).await {
            tracing::warn!(connector = %connector.id.0, %error, "queued IM replies could not be sent");
        }
    }
}
