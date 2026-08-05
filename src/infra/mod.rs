// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! MoleSignal 基础设施层。
//!
//! 各子模块都是 domain 端口（port）的具体实现（adapter）：
//!
//! - [`persistence`]   元数据库（sqlx + Postgres），实现各 repository trait
//! - [`storage`]       parquet + object_store，承担列式持久化
//! - [`search`]        datafusion 查询执行 + tantivy 倒排索引（占位）
//! - [`segment_wal`]   分段 Write-Ahead-Log，落地到本地磁盘
//! - [`notify`]        邮件 / Slack / Webhook 通知发送
//! - [`messaging`]     节点间或进程内消息总线
//! - [`ingest_sink`]   `IngestSink` 的 当前内存实现

pub mod alerting;
pub mod apm;
pub mod caching;
pub mod cipher;
pub mod cluster;
pub mod connectors;
pub mod enrichment;
pub mod ingest_sink;
pub mod ingester;
pub mod masking;
pub mod messaging;
pub mod notify;
pub mod persistence;
pub mod pipeline;
pub mod profiles;
pub mod query;
pub mod quotas;
pub mod reporting;
pub mod rum;
pub mod runtime;
pub mod search;
pub mod secret;
pub mod segment_wal;
pub mod sso;
pub mod storage;
pub mod traces;
