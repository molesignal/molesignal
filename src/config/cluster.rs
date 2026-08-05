// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 分布式拓扑：`[cluster]`（节点互联/心跳）、`[router]`（限流）。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterSettings {
    /// 节点对外宣告地址（gRPC 互联），写入 cluster_nodes 表
    #[serde(default = "default_advertise_addr")]
    pub advertise_addr: String,
    #[serde(default = "default_heartbeat_interval")]
    pub heartbeat_interval_secs: u32,
    #[serde(default = "default_peer_timeout")]
    pub peer_timeout_secs: u32,
}

fn default_advertise_addr() -> String {
    "127.0.0.1:5082".into()
}
fn default_heartbeat_interval() -> u32 {
    5
}
fn default_peer_timeout() -> u32 {
    15
}

impl Default for ClusterSettings {
    fn default() -> Self {
        Self {
            advertise_addr: default_advertise_addr(),
            heartbeat_interval_secs: default_heartbeat_interval(),
            peer_timeout_secs: default_peer_timeout(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RouterSettings {
    #[serde(default)]
    pub rate_limit: RouterRateLimit,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouterRateLimit {
    /// 每 org 每秒 ingest 请求上限；0 = 不限。
    #[serde(default = "default_ingest_qps")]
    pub ingest_qps: u32,
    /// 每 org 每秒查询请求上限；0 = 不限。
    #[serde(default = "default_query_qps")]
    pub query_qps: u32,
    /// 令牌桶突发容量倍数（实际容量 = qps * burst_multiplier）。
    #[serde(default = "default_rate_burst")]
    pub burst_multiplier: u32,
}

fn default_ingest_qps() -> u32 {
    1000
}
fn default_query_qps() -> u32 {
    100
}
fn default_rate_burst() -> u32 {
    2
}

impl Default for RouterRateLimit {
    fn default() -> Self {
        Self {
            ingest_qps: default_ingest_qps(),
            query_qps: default_query_qps(),
            burst_multiplier: default_rate_burst(),
        }
    }
}
