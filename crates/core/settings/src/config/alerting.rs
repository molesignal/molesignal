// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `[alert_manager]` 与 `[notify]` —— 告警评估/派发节奏与 SMTP 通知通道。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertManagerSettings {
    #[serde(default = "default_eval_interval")]
    pub eval_interval_secs: u32,
    #[serde(default = "default_dispatch_interval")]
    pub dispatch_interval_secs: u32,
    #[serde(default = "default_eval_timeout")]
    pub eval_timeout_secs: u32,
    #[serde(default = "default_ack_timeout")]
    pub default_ack_timeout_secs: u32,
}

fn default_eval_interval() -> u32 {
    30
}
fn default_dispatch_interval() -> u32 {
    10
}
fn default_eval_timeout() -> u32 {
    10
}
fn default_ack_timeout() -> u32 {
    300
}

impl Default for AlertManagerSettings {
    fn default() -> Self {
        Self {
            eval_interval_secs: default_eval_interval(),
            dispatch_interval_secs: default_dispatch_interval(),
            eval_timeout_secs: default_eval_timeout(),
            default_ack_timeout_secs: default_ack_timeout(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotifySettings {
    #[serde(default)]
    pub smtp: SmtpSettings,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SmtpTls {
    None,
    #[default]
    Starttls,
    Tls,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SmtpSettings {
    /// SMTP 启用 ⟺ `host` 非空（无独立开关）。
    #[serde(default)]
    pub host: String,
    #[serde(default)]
    pub port: u16,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub from: String,
    #[serde(default)]
    pub tls: SmtpTls,
    #[serde(default = "default_smtp_timeout")]
    pub timeout_secs: u32,
}

fn default_smtp_timeout() -> u32 {
    10
}
