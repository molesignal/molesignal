// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 跨节点的后台 worker 模块（feature-parity 各项 follow-up）。
//!
//! 这一层放在 bootstrap 而非 app：worker 通常需要 infra（repo 实装、object_store、
//! 配置 reload 钩子等），而 app crate 受架构约束不能依赖 infra。

pub mod admission_load_sync;
pub mod cluster;
pub mod parquet_file_meta_dumper;
pub mod pipeline_exec;
pub mod rca_sweeper;
pub mod rum_replay_retention;
pub mod scheduled_reports;
pub mod search_jobs;
pub mod service_graph;
pub mod slow_query_analyzer;
pub mod trial_sweeper;

pub mod acme;
