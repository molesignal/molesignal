// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Mole Agent 在 IM 里回答：飞书、Slack、钉钉、Google Chat、Microsoft Teams 的应用连接器与企业微信
//! 智能机器人收到的消息排成一轮回答，结束后回复到原处。
//!
//! - 设置：[`bots`]（哪些连接器回答、默认团队、长连接的状态）
//! - 收消息：[`inbound`]（对上提问人、IM 命令见 [`commands`]、话题对应的会话见 [`threads`]）
//! - 回复：[`reply`]（Run 结束时、worker 补发，以及连着企业微信的节点发别的节点交来的）
//! - 告警卡片上的确认、解决按钮：[`alerts`]；企业微信自建应用发的卡片上的点击：[`wecom_app`]
//! - 账号绑定：[`links`]
//! - Teams 的公网回调：[`teams`]
//!
//! 长连接由 worker `agent_im` 维持，协议在 `infra::im`。规则见 `agent::im`。

mod alerts;
mod bots;
mod commands;
mod inbound;
mod links;
mod reply;
mod teams;
mod texts;
mod threads;
mod wecom_app;

pub(crate) use alerts::{ImAlertButtons, act, keypress_answer};
use axum::Router;
pub(crate) use inbound::handle;
pub(crate) use reply::{drain, finished, reconcile};
pub use teams::public_routes;
pub(crate) use texts::TextSources;
pub(crate) use wecom_app::press as wecom_app_press;

use crate::api::AppState;

pub fn routes() -> Router<AppState> {
    Router::new().merge(bots::routes()).merge(links::routes())
}
