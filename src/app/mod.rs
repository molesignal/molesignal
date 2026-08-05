// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! MoleSignal 应用层。
//!
//! 每个子模块对应一个 domain 上下文，负责编排该上下文的 use case：
//! 接收命令 → 调用 domain 服务和 repository trait → 返回结果。
//!
//! app 层只依赖 domain 抽象，不直接依赖 infra 实现。
//! 具体绑定在 server crate 的启动阶段完成。

pub mod alerting;
pub mod apm;
pub mod cluster;
pub mod dashboard;
pub mod iam;
pub mod ingestion;
pub mod notify;
pub mod profile_storage;
pub mod profiling;
pub mod query;
pub mod recommendations;
pub mod search;
pub mod self_telemetry;
pub mod trace;
pub mod web;
