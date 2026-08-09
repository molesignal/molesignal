// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `[node]` —— 节点身份与承担的角色。

use serde::{Deserialize, Serialize};

use crate::shared::ids::Id;

const MAX_NODE_ID_CHARS: usize = 64;
const HOSTNAME_HASH_CHARS: usize = 16;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeSettings {
    #[serde(default = "default_roles")]
    pub roles: Vec<Role>,
    #[serde(default)]
    pub id: String,
    /// 收到 SIGTERM/SIGINT 后，等待 intake 把 pending 数据 flush 完（drained）的上限秒数；
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

impl NodeSettings {
    /// 显式配置优先；留空时使用 OS hostname，仅在 hostname 不可用时生成 KSUID。
    pub fn resolved_id(&self) -> String {
        let configured = self.id.trim();
        if !configured.is_empty() {
            return configured.to_owned();
        }
        let hostname = hostname::get()
            .ok()
            .map(|value| value.to_string_lossy().into_owned());
        resolve_node_id("", hostname.as_deref())
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        let id = self.id.trim();
        if id.chars().count() > MAX_NODE_ID_CHARS {
            anyhow::bail!("node.id must be at most {MAX_NODE_ID_CHARS} characters");
        }
        Ok(())
    }
}

fn resolve_node_id(configured: &str, hostname: Option<&str>) -> String {
    let configured = configured.trim();
    if !configured.is_empty() {
        return configured.to_owned();
    }

    hostname
        .and_then(normalize_hostname)
        .unwrap_or_else(|| Id::new().0)
}

fn normalize_hostname(hostname: &str) -> Option<String> {
    let hostname = hostname.trim().trim_end_matches('.');
    if hostname.is_empty() {
        return None;
    }
    if hostname.chars().count() <= MAX_NODE_ID_CHARS {
        return Some(hostname.to_owned());
    }

    let prefix_chars = MAX_NODE_ID_CHARS - HOSTNAME_HASH_CHARS - 1;
    let prefix: String = hostname.chars().take(prefix_chars).collect();
    let digest = blake3::hash(hostname.as_bytes()).to_hex();
    Some(format!(
        "{prefix}-{}",
        &digest.as_str()[..HOSTNAME_HASH_CHARS]
    ))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Standalone,
    Router,
    Intake,
    Querier,
    Compactor,
    AlertManager,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_node_id_takes_precedence_and_is_trimmed() {
        assert_eq!(
            resolve_node_id("  configured-node  ", Some("host-node")),
            "configured-node"
        );
    }

    #[test]
    fn hostname_is_the_default_node_id() {
        assert_eq!(resolve_node_id("", Some("intake-0")), "intake-0");
        assert_eq!(resolve_node_id("  ", Some("intake-0.")), "intake-0");
    }

    #[test]
    fn missing_hostname_falls_back_to_ksuid() {
        let node_id = resolve_node_id("", None);
        assert_eq!(node_id.len(), 27);
    }

    #[test]
    fn long_hostname_is_bounded_and_deterministic() {
        let hostname = "node.".repeat(20);
        let first = resolve_node_id("", Some(&hostname));
        let second = resolve_node_id("", Some(&hostname));
        assert_eq!(first, second);
        assert_eq!(first.chars().count(), MAX_NODE_ID_CHARS);
    }

    #[test]
    fn configured_node_id_must_fit_persistence_columns() {
        let settings = NodeSettings {
            id: "n".repeat(MAX_NODE_ID_CHARS + 1),
            ..NodeSettings::default()
        };
        assert!(settings.validate().is_err());
    }
}
