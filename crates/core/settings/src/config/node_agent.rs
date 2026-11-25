// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `[node_agent]`: the managed-mode control endpoint of Node Agents (`NodeControl` over gRPC).
//!
//! One TLS listener serves `Enroll` without a client certificate, and `Session` and
//! `RotateCertificate` only to a client certificate of the Node Agent CA.

use serde::{Deserialize, Serialize};

use super::yes;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeAgentControlSettings {
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default = "default_bind")]
    pub bind: String,
    #[serde(default = "default_port")]
    pub port: u16,
    /// The URL agents connect to, shown with every new enrollment token. Empty: `https://localhost:<port>`.
    #[serde(default)]
    pub endpoint: String,
    /// DNS names and IPs in the server certificate; agents check the endpoint host against them.
    #[serde(default = "default_server_names")]
    pub server_names: Vec<String>,
    #[serde(default = "default_certificate_days")]
    pub certificate_days: u32,
    /// Largest message, in MiB.
    #[serde(default = "default_max_message_size")]
    pub max_message_size: u32,
    #[serde(default = "default_heartbeat_interval_secs")]
    pub heartbeat_interval_secs: u32,
    /// How long a lease lasts without a renewal. Heartbeats renew it, so it spans a few of them.
    #[serde(default = "default_lease_ttl_secs")]
    pub lease_ttl_secs: u32,
    /// How long an offer waits for the node to accept it before it is offered again.
    #[serde(default = "default_offer_ttl_secs")]
    pub offer_ttl_secs: u32,
}

fn default_bind() -> String {
    "0.0.0.0".into()
}

const fn default_port() -> u16 {
    5086
}

fn default_server_names() -> Vec<String> {
    vec!["localhost".to_string(), "127.0.0.1".to_string()]
}

const fn default_certificate_days() -> u32 {
    30
}

const fn default_max_message_size() -> u32 {
    4
}

const fn default_heartbeat_interval_secs() -> u32 {
    30
}

const fn default_lease_ttl_secs() -> u32 {
    120
}

const fn default_offer_ttl_secs() -> u32 {
    60
}

impl Default for NodeAgentControlSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            bind: default_bind(),
            port: default_port(),
            endpoint: String::new(),
            server_names: default_server_names(),
            certificate_days: default_certificate_days(),
            max_message_size: default_max_message_size(),
            heartbeat_interval_secs: default_heartbeat_interval_secs(),
            lease_ttl_secs: default_lease_ttl_secs(),
            offer_ttl_secs: default_offer_ttl_secs(),
        }
    }
}

impl NodeAgentControlSettings {
    pub fn resolved_endpoint(&self) -> String {
        if self.endpoint.trim().is_empty() {
            format!("https://localhost:{}", self.port)
        } else {
            self.endpoint.trim().trim_end_matches('/').to_string()
        }
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        if !self.resolved_endpoint().starts_with("https://") {
            anyhow::bail!("node_agent endpoint must use https://");
        }

        if self.server_names.is_empty() {
            anyhow::bail!("node_agent server_names must contain at least one DNS name or IP");
        }

        if self.certificate_days < 2 || self.certificate_days > 365 {
            anyhow::bail!("node_agent certificate_days must be between 2 and 365");
        }

        if !(5..=600).contains(&self.heartbeat_interval_secs) {
            anyhow::bail!("node_agent heartbeat_interval_secs must be between 5 and 600");
        }

        // A lease has to outlive at least two missed heartbeats.
        if self.lease_ttl_secs < self.heartbeat_interval_secs.saturating_mul(3)
            || self.lease_ttl_secs > 3600
        {
            anyhow::bail!(
                "node_agent lease_ttl_secs must be at least three heartbeat intervals and at most 3600"
            );
        }

        if !(10..=3600).contains(&self.offer_ttl_secs) {
            anyhow::bail!("node_agent offer_ttl_secs must be between 10 and 3600");
        }

        if self.max_message_size == 0 || self.max_message_size > 64 {
            anyhow::bail!("node_agent max_message_size must be between 1 and 64 MiB");
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::NodeAgentControlSettings;

    #[test]
    fn defaults_validate_and_a_short_lease_does_not() {
        let mut settings = NodeAgentControlSettings::default();
        settings.validate().expect("defaults validate");
        assert_eq!(settings.resolved_endpoint(), "https://localhost:5086");

        settings.lease_ttl_secs = settings.heartbeat_interval_secs * 2;
        assert!(settings.validate().is_err());
    }
}
