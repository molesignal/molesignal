// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 自动化的应用层：保存规则时的校验、Cron 触发的下一次、API 令牌、内置模板，以及一次运行
//! 结束时的状态与通知文案。
//!
//! 开始与结束一次运行要用到会话、Run、通知与事件，那些在 `api::http::routes::agent::automations`；
//! 定时与事件触发的循环在 `bootstrap::workers::agent_automations`。

pub mod outcome;
pub mod schedule;
pub mod templates;
pub mod token;
pub mod validate;
