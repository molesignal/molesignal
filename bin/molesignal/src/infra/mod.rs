// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! MoleSignal 基础设施层。
//!
//! 各子模块都是 domain 端口（port）的具体实现（adapter）：
//!
//! - [`storage`]       parquet + object_store，承担列式持久化
//! - [`search`]        datafusion 查询执行 + tantivy 倒排索引（占位）
//! - [`notify`]        邮件 / Slack / Webhook 通知发送
//! - [`im`]            Mole Agent 在 IM 里：飞书、Slack、钉钉的长连接与回复
//! - [`messaging`]     节点间或进程内消息总线
//! - [`intake_sink`]   `IntakeSink` 的 当前内存实现

pub mod apm;
pub mod caching;
pub mod cluster;
pub mod connectors;
pub mod enrichment;
pub mod im;
pub mod intake;
pub mod intake_sink;
pub mod mailbox;
pub mod masking;
pub mod messaging;
pub mod mfa;
pub mod notify;
pub mod pipeline;
pub mod query;
pub mod releases;
pub mod reporting;
pub mod rum;
pub mod search;
pub mod secret;
pub mod signin;
pub mod sso;
pub mod status_page;
pub mod storage;
pub mod traces;
