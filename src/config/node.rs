// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `[node]` —— 节点身份与承担的角色。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeSettings {
    #[serde(default = "default_roles")]
    pub roles: Vec<Role>,
    #[serde(default)]
    pub id: String,
    /// 收到 SIGTERM/SIGINT 后，等待 ingester 把 pending 数据 flush 完（drained）的上限秒数；
    /// 超时则强制退出。
    #[serde(default = "default_drain_timeout")]
    pub drain_timeout_secs: u32,
}

fn default_roles() -> Vec<Role> {
    vec![Role::Standalone]
}

fn default_drain_timeout() -> u32 {
    30
}

impl Default for NodeSettings {
    fn default() -> Self {
        Self {
            roles: default_roles(),
            id: String::new(),
            drain_timeout_secs: default_drain_timeout(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Standalone,
    Router,
    Ingester,
    Querier,
    Compactor,
    AlertManager,
}
