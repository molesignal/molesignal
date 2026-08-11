// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde::{Deserialize, Serialize};

use super::{HeaderValue, MonitorAssertion, ValueSource};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TcpSpec {
    pub host: ValueSource,
    pub port: u16,
    pub use_tls: bool,
    pub server_name: Option<String>,
    pub send: Option<ValueSource>,
    pub expect_regex: Option<String>,
    #[serde(default)]
    pub assertions: Vec<MonitorAssertion>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DnsSpec {
    pub name: String,
    pub record_type: String,
    pub resolver: Option<String>,
    pub require_dnssec: bool,
    #[serde(default)]
    pub expected_values: Vec<String>,
    pub expected_rcode: Option<String>,
    #[serde(default)]
    pub assertions: Vec<MonitorAssertion>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IcmpSpec {
    pub host: String,
    pub count: u8,
    pub interval_millis: u32,
    pub max_packet_loss_ratio: f64,
    pub max_mean_rtt_millis: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TlsSpec {
    pub host: String,
    pub port: u16,
    pub server_name: Option<String>,
    pub minimum_days_remaining: u32,
    #[serde(default)]
    pub expected_sans: Vec<String>,
    pub expected_issuer_regex: Option<String>,
    pub minimum_protocol: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GrpcCall {
    Health {
        service: String,
    },
    Unary {
        service: String,
        method: String,
        descriptor_set_base64: Option<String>,
        use_reflection: bool,
        request_json: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GrpcSpec {
    pub endpoint: String,
    pub use_tls: bool,
    pub server_name: Option<String>,
    #[serde(default)]
    pub metadata: Vec<HeaderValue>,
    pub call: GrpcCall,
    #[serde(default)]
    pub assertions: Vec<MonitorAssertion>,
}
