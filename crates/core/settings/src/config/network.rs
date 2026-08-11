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
    /// 访问来源（`window.location.origin`）推导；非空时数据源接入页等展示用它，
    /// 同时作为 Status Page 自定义域名验证的 CNAME 路由目标。
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

/// `[probe]`: public TLS registration and private mTLS control listeners for Probe Agents.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProbeGrpcSettings {
    #[serde(default = "default_http_bind")]
    pub bind: String,
    #[serde(default = "default_probe_register_port")]
    pub register_port: u16,
    #[serde(default = "default_probe_control_port")]
    pub control_port: u16,
    #[serde(default)]
    pub register_endpoint: String,
    #[serde(default)]
    pub control_endpoint: String,
    #[serde(default = "default_probe_server_names")]
    pub server_names: Vec<String>,
    #[serde(default = "default_probe_certificate_days")]
    pub certificate_days: u32,
    #[serde(default = "default_grpc_msg")]
    pub max_message_size_mb: u32,
}

const fn default_probe_register_port() -> u16 {
    5084
}

const fn default_probe_control_port() -> u16 {
    5085
}

fn default_probe_server_names() -> Vec<String> {
    vec!["localhost".to_string(), "127.0.0.1".to_string()]
}

const fn default_probe_certificate_days() -> u32 {
    30
}

impl ProbeGrpcSettings {
    pub fn resolved_register_endpoint(&self) -> String {
        if self.register_endpoint.trim().is_empty() {
            format!("https://localhost:{}", self.register_port)
        } else {
            self.register_endpoint.trim_end_matches('/').to_string()
        }
    }

    pub fn resolved_control_endpoint(&self) -> String {
        if self.control_endpoint.trim().is_empty() {
            format!("https://localhost:{}", self.control_port)
        } else {
            self.control_endpoint.trim_end_matches('/').to_string()
        }
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        if self.register_port == self.control_port {
            anyhow::bail!("probe register_port and control_port must differ");
        }
        if self.certificate_days == 0 || self.certificate_days > 365 {
            anyhow::bail!("probe certificate_days must be between 1 and 365");
        }
        for endpoint in [
            self.resolved_register_endpoint(),
            self.resolved_control_endpoint(),
        ] {
            if !endpoint.starts_with("https://") {
                anyhow::bail!("Probe endpoints must use https://");
            }
        }
        if self.server_names.is_empty() {
            anyhow::bail!("probe server_names must contain at least one DNS name or IP");
        }
        Ok(())
    }
}

impl Default for ProbeGrpcSettings {
    fn default() -> Self {
        Self {
            bind: default_http_bind(),
            register_port: default_probe_register_port(),
            control_port: default_probe_control_port(),
            register_endpoint: String::new(),
            control_endpoint: String::new(),
            server_names: default_probe_server_names(),
            certificate_days: default_probe_certificate_days(),
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
/// 与 `[grpc]`（集群内可信网络、`intake.v1` 私有分发协议）分端口：本端口挂标准 OTLP
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
