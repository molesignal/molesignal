// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 跨节点的后台 worker 模块。
//!
//! 这一层放在 bootstrap 而非 app：worker 通常需要 infra（repo 实装、object_store、
//! 配置 reload 钩子等），而 app crate 受架构约束不能依赖 infra。

pub mod admission_load_sync;
pub mod agent_automations;
pub mod agent_im;
pub mod agent_mcp;
pub mod agent_runs;
pub mod agent_scoring;
pub mod agent_skills;
pub mod agent_traces;
pub mod audit_retention;
pub mod audit_stream;
pub mod cluster;
pub mod intake_usage;
pub mod jwt_secrets;
pub mod log_inspection;
pub mod pipeline_exec;
pub mod pipeline_steps;
pub mod polling;
pub mod rca_sweeper;
pub mod rum_dashboards;
pub mod rum_replay_retention;
pub mod scheduled_reports;
pub mod search_jobs;
pub mod series_limits;
pub mod service_graph;
pub mod slow_query_analyzer;
pub mod status_page_dependencies;
pub mod status_page_maintenance;
pub mod status_page_notifications;
pub mod status_page_planned_maintenance;
pub mod storage_maintenance;
pub mod synthetic_results;
pub mod synthetic_retention;
pub mod synthetics;

pub mod acme;
