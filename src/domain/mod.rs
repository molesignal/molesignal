// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! MoleSignal 领域层。
//!
//! 按 DDD 划分为以下限界上下文，每个上下文都是一个独立子模块：
//!
//! - [`ingestion`]  采集上下文：日志/指标/Trace 的写入领域模型
//! - [`query`]      查询上下文：查询请求、结果、查询计划元数据
//! - [`alerting`]   告警上下文：告警规则、事件、排班、升级策略
//! - [`dashboard`]  仪表盘上下文：MoleSignal 原生 Dashboard 模型
//! - [`iam`]        IAM 上下文：身份、组织、角色、权限
//! - [`storage`]    存储上下文：parquet 文件元信息、分区
//! - [`stream`]     流上下文：数据流定义、Schema、保留策略
//!
//! 本 crate 严格不依赖任何具体基础设施（数据库、对象存储等），
//! 所有外部能力以 trait 形式声明在各上下文的 `repositories` 模块中。

pub mod alerting;
pub mod apm;
pub mod billing;
pub mod dashboard;
pub mod federation;
pub mod function;
pub mod iam;
pub mod ingestion;
pub mod license;
pub mod masking;
pub mod metrics;
pub mod notify;
pub mod pipeline;
pub mod query;
pub mod rum;
pub mod saved_view;
pub mod storage;
pub mod stream;
pub mod trace_policy;
