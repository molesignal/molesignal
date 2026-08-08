// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 各 domain repository trait 的 sqlx 实现。
//!
//! 每个模块只覆盖 happy-path CRUD；按时间窗、按 fingerprint 等查询走原生 SQL，
//! JSON 列经 `sqlx::types::Json` / `serde_json::Value` 中转到 domain 结构。

pub mod alert_rules;
pub mod annotations;
pub mod api_tokens;
pub mod audit_events;
pub mod billing_settings;
pub mod cluster;
pub mod dashboard_authoring;
pub mod dashboard_contract_registry;
pub mod dashboards;
pub mod debug_artifacts;
pub mod domains;
pub mod email_domains;
pub mod escalation_policies;
pub mod field_masking_rules;
pub mod file_download_tokens;
pub mod folders;
pub mod functions;
pub mod iam;
pub mod incidents;
pub mod instance_settings;
pub mod intelligence;
pub mod investigation_blobs;
pub mod invitations;
pub mod license_versions;
pub mod log_patterns;
pub mod marketplace;
pub mod model_prices;
pub mod mute_rules;
pub mod notify;
pub mod organizations;
pub mod parquet_file_meta;
pub mod password_resets;
pub mod pipelines;
pub mod quotas;
pub mod regex_patterns;
pub mod report_templates;
pub mod resource_shares;
pub mod saved_views;
pub mod scheduled_reports;
pub mod schedules;
pub mod search;
pub mod semantic_groups;
pub mod signing_secrets;
pub mod slow_queries;
pub mod sso_providers;
pub mod streams;
pub mod teams;
pub mod trace_policies;
pub mod trials;
pub mod usage;
pub mod user_preferences;
pub mod users;
pub mod web_search;
pub mod workspace_preference_defaults;

pub(crate) use super::sqlx_err;
