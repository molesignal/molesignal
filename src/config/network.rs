// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 对外/对内网络端口：`[http]`（含 `[http.tls]`）、`[grpc]`、`[flight_sql]`、`[otlp_grpc]`。

use serde::{Deserialize, Serialize};

use super::yes;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpSettings {
    #[serde(default = "default_http_bind")]
    pub bind: String,
    #[serde(default = "default_http_port")]
    pub port: u16,
    #[serde(default = "yes")]
    pub gzip: bool,
    /// TLS + ACME 自动证书（change `domain-acme-tls`）。
    /// `enabled = false` 时整段忽略，等价于现状（单 plain HTTP）。
    #[serde(default)]
    pub tls: TlsSettings,
    /// 对外访问 URL（如反代后的 `https://obs.example.com`）。留空时前端按当前
    /// 访问来源（`window.location.origin`）推导；非空时数据源接入页等展示用它。
    /// env 覆盖：`MS_HTTP_EXTERNAL_URL`。
    #[serde(default)]
    pub external_url: String,
}

fn default_http_bind() -> String {
    "0.0.0.0".into()
}
fn default_http_port() -> u16 {
    5080
}

impl Default for HttpSettings {
    fn default() -> Self {
        Self {
            bind: default_http_bind(),
            port: default_http_port(),
            gzip: true,
            tls: TlsSettings::default(),
            external_url: String::new(),
        }
    }
}

/// `[http.tls]` —— ACME + rustls 服务端配置（仅  feature 实际启用）。
///
/// 默认 `enabled=false` 时所有字段忽略；OSS / 不需要 TLS 的部署不受影响。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TlsSettings {
    /// 主开关；false 时其他字段全部忽略，与 disabled 等价。
    #[serde(default)]
    pub enabled: bool,
    /// 80 端口替代（HTTP-01 challenge + redirect）。
    #[serde(default = "default_tls_plain_port")]
    pub plain_port: u16,
    /// 443 端口替代（rustls + SNI cert）。
    #[serde(default = "default_tls_port")]
    pub port: u16,
    /// ACME directory：`production` / `staging` / 任意 URL（Pebble for tests）。
    #[serde(default = "default_acme_directory")]
    pub acme_directory: String,
    /// 账户邮箱（LetsEncrypt 要求；TOS 通知发到这里）。
    #[serde(default)]
    pub account_email: String,
    /// ACME account key + 每域 `*.key.pem` 落盘位置。
    #[serde(default = "default_key_storage_dir")]
    pub key_storage_dir: String,
    /// `acme_runner` 扫 pending 间隔（秒）。
    #[serde(default = "default_issue_poll_secs")]
    pub issue_poll_secs: u64,
    /// renewal 重试 / 临期扫间隔（秒）。
    #[serde(default = "default_renewal_retry_secs")]
    pub renewal_retry_secs: u64,
}

fn default_tls_plain_port() -> u16 {
    80
}
fn default_tls_port() -> u16 {
    443
}
fn default_acme_directory() -> String {
    "production".into()
}
fn default_key_storage_dir() -> String {
    "/var/lib/molesignal/acme".into()
}
fn default_issue_poll_secs() -> u64 {
    60
}
fn default_renewal_retry_secs() -> u64 {
    6 * 3600
}

impl Default for TlsSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            plain_port: default_tls_plain_port(),
            port: default_tls_port(),
            acme_directory: default_acme_directory(),
            account_email: String::new(),
            key_storage_dir: default_key_storage_dir(),
            issue_poll_secs: default_issue_poll_secs(),
            renewal_retry_secs: default_renewal_retry_secs(),
        }
    }
}

impl TlsSettings {
    /// 解析 directory string 成完整 URL：
    /// - `production` → LetsEncrypt 生产
    /// - `staging` → LetsEncrypt staging（测试用，rate limit 宽松）
    /// - 其他 → 直接作为 URL 用（Pebble 本地 CA / 自部署）
    pub fn directory_url(&self) -> &str {
        match self.acme_directory.as_str() {
            "production" => "https://acme-v02.api.letsencrypt.org/directory",
            "staging" => "https://acme-staging-v02.api.letsencrypt.org/directory",
            other => other,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrpcSettings {
    #[serde(default = "default_http_bind")]
    pub bind: String,
    #[serde(default = "default_grpc_port")]
    pub port: u16,
    #[serde(default = "default_grpc_msg")]
    pub max_message_size_mb: u32,
}

fn default_grpc_port() -> u16 {
    5082
}
fn default_grpc_msg() -> u32 {
    32
}

impl Default for GrpcSettings {
    fn default() -> Self {
        Self {
            bind: default_http_bind(),
            port: default_grpc_port(),
            max_message_size_mb: default_grpc_msg(),
        }
    }
}

/// `[flight_sql]`（spec `flight-sql`）：对外 Arrow Flight SQL listener。
///
/// 与 `[grpc]`（集群内可信网络，含免鉴权 shard 协议）分端口：本端口上每个
/// RPC 都强制 API token 鉴权，可暴露给用户网络；默认关闭。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlightSqlSettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_http_bind")]
    pub bind: String,
    #[serde(default = "default_flight_sql_port")]
    pub port: u16,
    /// SQL 未携带分区裁剪信息时的缺省回看窗口（`now - N 小时 .. now`）。
    #[serde(default = "default_flight_sql_lookback_hours")]
    pub default_lookback_hours: u32,
    #[serde(default = "default_grpc_msg")]
    pub max_message_size_mb: u32,
}

fn default_flight_sql_port() -> u16 {
    5083
}
fn default_flight_sql_lookback_hours() -> u32 {
    24
}

impl Default for FlightSqlSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            bind: default_http_bind(),
            port: default_flight_sql_port(),
            default_lookback_hours: default_flight_sql_lookback_hours(),
            max_message_size_mb: default_grpc_msg(),
        }
    }
}

/// `[otlp_grpc]`：对外**标准 OTLP gRPC** receiver（traces/logs/metrics/profiles）。
///
/// 与 `[grpc]`（集群内可信网络、`ingest.v1` 私有分发协议）分端口：本端口挂标准 OTLP
/// collector service，每个 RPC 强制 Bearer 鉴权 + `StreamWrite`，可暴露给用户网络；
/// 始终监听标准 `:4317`。这是对外端口，生产请配 TLS。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OtlpGrpcSettings {
    #[serde(default = "default_http_bind")]
    pub bind: String,
    #[serde(default = "default_otlp_grpc_port")]
    pub port: u16,
    #[serde(default = "default_grpc_msg")]
    pub max_message_size_mb: u32,
}

fn default_otlp_grpc_port() -> u16 {
    4317
}

impl Default for OtlpGrpcSettings {
    fn default() -> Self {
        Self {
            bind: default_http_bind(),
            port: default_otlp_grpc_port(),
            max_message_size_mb: default_grpc_msg(),
        }
    }
}

/// `[syslog]`：裸 UDP/TCP syslog 接入（RFC3164 / RFC5424）。**无鉴权** —— 必须显式绑定 org。
/// 全部字段默认空 → 整段禁用（OSS / 现状零行为）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SyslogSettings {
    /// UDP 监听地址（如 `0.0.0.0:5514`）；空 = 不起 UDP。
    #[serde(default)]
    pub udp_bind: String,
    /// TCP 监听地址（如 `0.0.0.0:5514`，换行分帧）；空 = 不起 TCP。
    #[serde(default)]
    pub tcp_bind: String,
    /// 写入目标 org 的 slug。syslog 无鉴权、无 org 上下文，必须显式绑定；空 = 整段禁用。
    #[serde(default)]
    pub org: String,
    /// 目标 stream 名（空回退到 `syslog`）。
    #[serde(default)]
    pub stream: String,
}
