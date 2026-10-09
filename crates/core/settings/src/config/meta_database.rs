// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `[meta_database]` —— 元数据库（PostgreSQL）连接与连接池边界。

use serde::{Deserialize, Serialize};

/// `[meta_database]`：元数据库连接。
///
/// 拒绝未知键：写错的键名会让启动失败。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetaDatabaseSettings {
    #[serde(default = "default_meta_dsn")]
    pub dsn: String,
    #[serde(default = "default_meta_min_conns")]
    pub min_connections: u32,
    #[serde(default = "default_meta_conns")]
    pub max_connections: u32,
}

fn default_meta_dsn() -> String {
    "postgres://molesignal:molesignal@localhost:5432/molesignal".into()
}
fn default_meta_min_conns() -> u32 {
    2
}
fn default_meta_conns() -> u32 {
    16
}

impl Default for MetaDatabaseSettings {
    fn default() -> Self {
        Self {
            dsn: default_meta_dsn(),
            min_connections: default_meta_min_conns(),
            max_connections: default_meta_conns(),
        }
    }
}
