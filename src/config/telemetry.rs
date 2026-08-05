// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `[telemetry]` / `[profiling]` —— 进程日志、OTLP、自遥测回灌与 pprof listener。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

mod self_collect;

pub use self_collect::SelfCollectSettings;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TelemetrySettings {
    #[serde(default = "default_log_level")]
    pub log_level: String,
    #[serde(default = "default_log_format")]
    pub log_format: String,
    #[serde(default = "default_log_output")]
    pub log_output: String,
    #[serde(default)]
    pub log_directory: Option<String>,
    #[serde(default)]
    pub log_file_prefix: Option<String>,
    #[serde(default)]
    pub log_rotation: Option<String>,
    #[serde(default)]
    pub log_max_files: Option<usize>,
    #[serde(default)]
    pub self_collect: SelfCollectSettings,
    #[serde(default)]
    pub trace: TraceSettings,
}

fn default_log_level() -> String {
    "info".into()
}
fn default_log_format() -> String {
    "text".into()
}
fn default_log_output() -> String {
    "console".into()
}

impl Default for TelemetrySettings {
    fn default() -> Self {
        Self {
            log_level: default_log_level(),
            log_format: default_log_format(),
            log_output: default_log_output(),
            log_directory: None,
            log_file_prefix: None,
            log_rotation: None,
            log_max_files: None,
            self_collect: SelfCollectSettings::default(),
            trace: TraceSettings::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TraceSettings {
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// 部署级最高优先级开关，运行时策略不可覆盖。
    #[serde(default)]
    pub force_disabled: bool,
    #[serde(default = "default_trace_filter")]
    pub filter: String,
    #[serde(default = "default_deployment_environment")]
    pub deployment_environment: String,
    #[serde(default = "default_normal_sample_ratio")]
    pub normal_sample_ratio: f64,
    #[serde(default = "default_development_sample_ratio")]
    pub development_sample_ratio: f64,
    #[serde(default = "default_decision_window_secs")]
    pub decision_window_secs: u64,
    #[serde(default = "default_root_grace_millis")]
    pub root_grace_millis: u64,
    #[serde(default = "default_decision_cache_secs")]
    pub decision_cache_secs: u64,
    #[serde(default = "default_tail_max_traces")]
    pub tail_max_traces: usize,
    #[serde(default = "default_tail_memory_bytes")]
    pub tail_memory_bytes: usize,
    #[serde(default = "default_trace_max_spans")]
    pub max_spans_per_trace: usize,
    #[serde(default = "default_trace_max_attributes")]
    pub max_attributes_per_span: usize,
    #[serde(default = "default_trace_max_events")]
    pub max_events_per_span: usize,
    #[serde(default = "default_trace_max_links")]
    pub max_links_per_span: usize,
    #[serde(default = "default_trace_max_string_bytes")]
    pub max_string_bytes: usize,
    #[serde(default = "default_trace_shutdown_timeout_secs")]
    pub shutdown_timeout_secs: u64,
    #[serde(default)]
    pub slow_thresholds: TraceSlowThresholds,
    #[serde(default)]
    pub external: ExternalTraceExporterSettings,
}

impl Default for TraceSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            force_disabled: false,
            filter: default_trace_filter(),
            deployment_environment: default_deployment_environment(),
            normal_sample_ratio: default_normal_sample_ratio(),
            development_sample_ratio: default_development_sample_ratio(),
            decision_window_secs: default_decision_window_secs(),
            root_grace_millis: default_root_grace_millis(),
            decision_cache_secs: default_decision_cache_secs(),
            tail_max_traces: default_tail_max_traces(),
            tail_memory_bytes: default_tail_memory_bytes(),
            max_spans_per_trace: default_trace_max_spans(),
            max_attributes_per_span: default_trace_max_attributes(),
            max_events_per_span: default_trace_max_events(),
            max_links_per_span: default_trace_max_links(),
            max_string_bytes: default_trace_max_string_bytes(),
            shutdown_timeout_secs: default_trace_shutdown_timeout_secs(),
            slow_thresholds: TraceSlowThresholds::default(),
            external: ExternalTraceExporterSettings::default(),
        }
    }
}

impl TraceSettings {
    pub fn effective_enabled(&self) -> bool {
        self.enabled && !self.force_disabled
    }

    pub fn effective_normal_sample_ratio(&self) -> f64 {
        if matches!(
            self.deployment_environment.to_ascii_lowercase().as_str(),
            "development" | "dev" | "test"
        ) {
            self.development_sample_ratio
        } else {
            self.normal_sample_ratio
        }
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        if self.filter.trim().is_empty() {
            anyhow::bail!("telemetry.trace.filter must not be empty");
        }
        tracing_subscriber::EnvFilter::try_new(&self.filter)
            .map_err(|error| anyhow::anyhow!("invalid telemetry.trace.filter: {error}"))?;
        for (field, value) in [
            ("normal_sample_ratio", self.normal_sample_ratio),
            ("development_sample_ratio", self.development_sample_ratio),
        ] {
            if !value.is_finite() || !(0.0..=1.0).contains(&value) {
                anyhow::bail!("telemetry.trace.{field} must be between 0 and 1");
            }
        }
        if !(5..=120).contains(&self.decision_window_secs) {
            anyhow::bail!("telemetry.trace.decision_window_secs must be between 5 and 120");
        }
        if self.root_grace_millis == 0
            || self.decision_cache_secs < self.decision_window_secs
            || self.tail_max_traces == 0
            || self.tail_memory_bytes == 0
            || self.max_spans_per_trace == 0
            || self.max_attributes_per_span == 0
            || self.max_events_per_span == 0
            || self.max_links_per_span == 0
            || self.max_string_bytes == 0
            || self.shutdown_timeout_secs == 0
        {
            anyhow::bail!("telemetry.trace limits and timeouts must be greater than zero");
        }
        self.slow_thresholds.validate()?;
        self.external.validate()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceSlowThresholds {
    #[serde(default = "default_http_slow_ms")]
    pub http_ms: u64,
    #[serde(default = "default_query_slow_ms")]
    pub query_ms: u64,
    #[serde(default = "default_ingest_slow_ms")]
    pub batch_ingest_ms: u64,
    #[serde(default = "default_database_slow_ms")]
    pub database_ms: u64,
    #[serde(default = "default_object_store_slow_ms")]
    pub object_store_ms: u64,
    #[serde(default = "default_external_slow_ms")]
    pub external_ms: u64,
    #[serde(default = "default_background_slow_ms")]
    pub background_ms: u64,
}

impl Default for TraceSlowThresholds {
    fn default() -> Self {
        Self {
            http_ms: default_http_slow_ms(),
            query_ms: default_query_slow_ms(),
            batch_ingest_ms: default_ingest_slow_ms(),
            database_ms: default_database_slow_ms(),
            object_store_ms: default_object_store_slow_ms(),
            external_ms: default_external_slow_ms(),
            background_ms: default_background_slow_ms(),
        }
    }
}

impl TraceSlowThresholds {
    fn validate(&self) -> anyhow::Result<()> {
        if [
            self.http_ms,
            self.query_ms,
            self.batch_ingest_ms,
            self.database_ms,
            self.object_store_ms,
            self.external_ms,
            self.background_ms,
        ]
        .contains(&0)
        {
            anyhow::bail!("telemetry.trace.slow_thresholds values must be greater than zero");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalTraceExporterSettings {
    #[serde(default)]
    pub endpoint: String,
    #[serde(default = "default_otlp_protocol")]
    pub protocol: String,
    #[serde(default = "default_export_timeout_ms")]
    pub timeout_ms: u64,
    #[serde(default = "default_export_queue_capacity")]
    pub queue_capacity: usize,
    #[serde(default = "default_export_batch_size")]
    pub batch_size: usize,
    #[serde(default)]
    pub gzip: bool,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default)]
    pub custom_ca_file: Option<String>,
    #[serde(default)]
    pub client_certificate_file: Option<String>,
    #[serde(default)]
    pub client_key_file: Option<String>,
    #[serde(default)]
    pub allow_self_export: bool,
}

impl Default for ExternalTraceExporterSettings {
    fn default() -> Self {
        Self {
            endpoint: String::new(),
            protocol: default_otlp_protocol(),
            timeout_ms: default_export_timeout_ms(),
            queue_capacity: default_export_queue_capacity(),
            batch_size: default_export_batch_size(),
            gzip: false,
            headers: BTreeMap::new(),
            custom_ca_file: None,
            client_certificate_file: None,
            client_key_file: None,
            allow_self_export: false,
        }
    }
}

impl ExternalTraceExporterSettings {
    fn validate(&self) -> anyhow::Result<()> {
        if !matches!(self.protocol.as_str(), "grpc" | "http/protobuf") {
            anyhow::bail!("telemetry.trace.external.protocol must be `grpc` or `http/protobuf`");
        }
        if self.endpoint.is_empty() {
            return Ok(());
        }
        let url = url::Url::parse(&self.endpoint)
            .map_err(|error| anyhow::anyhow!("invalid Trace OTLP endpoint: {error}"))?;
        if !matches!(url.scheme(), "http" | "https") {
            anyhow::bail!("Trace OTLP endpoint must use http or https");
        }
        if self.timeout_ms == 0 || self.queue_capacity == 0 || self.batch_size == 0 {
            anyhow::bail!("Trace exporter timeout, queue, and batch values must be non-zero");
        }
        if self.client_certificate_file.is_some() != self.client_key_file.is_some() {
            anyhow::bail!("Trace exporter mTLS certificate and key must be configured together");
        }
        for (name, value) in &self.headers {
            if name.trim().is_empty() || value.trim().is_empty() {
                anyhow::bail!("Trace exporter headers must not contain empty names/values");
            }
            let sensitive = matches!(
                name.to_ascii_lowercase().as_str(),
                "authorization" | "proxy-authorization" | "x-api-key"
            );
            if sensitive && !(value.starts_with("env:") || value.starts_with("secret:")) {
                anyhow::bail!("sensitive Trace exporter header `{name}` must use env: or secret:");
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfilingSettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_profiling_bind")]
    pub bind: String,
    #[serde(default = "default_profiling_port")]
    pub port: u16,
    #[serde(default)]
    pub allow_remote: bool,
}

impl Default for ProfilingSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            bind: default_profiling_bind(),
            port: default_profiling_port(),
            allow_remote: false,
        }
    }
}

impl ProfilingSettings {
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.enabled && self.bind.trim().is_empty() {
            anyhow::bail!("profiling.bind must not be empty when profiling is enabled");
        }
        if self.enabled && self.port == 0 {
            anyhow::bail!("profiling.port must be greater than zero when profiling is enabled");
        }
        if self.enabled {
            let bind = self.bind.parse::<std::net::IpAddr>().map_err(|error| {
                anyhow::anyhow!("profiling.bind must be an IP address: {error}")
            })?;
            if !self.allow_remote && !bind.is_loopback() {
                anyhow::bail!("profiling.bind must be loopback unless profiling.allow_remote=true");
            }
        }
        Ok(())
    }
}

fn default_true() -> bool {
    true
}
fn default_trace_filter() -> String {
    "info".into()
}
fn default_deployment_environment() -> String {
    "production".into()
}
fn default_normal_sample_ratio() -> f64 {
    0.1
}
fn default_development_sample_ratio() -> f64 {
    1.0
}
fn default_decision_window_secs() -> u64 {
    30
}
fn default_root_grace_millis() -> u64 {
    1_000
}
fn default_decision_cache_secs() -> u64 {
    300
}
fn default_tail_max_traces() -> usize {
    10_000
}
fn default_tail_memory_bytes() -> usize {
    256 * 1024 * 1024
}
fn default_trace_max_spans() -> usize {
    1_000
}
fn default_trace_max_attributes() -> usize {
    128
}
fn default_trace_max_events() -> usize {
    128
}
fn default_trace_max_links() -> usize {
    128
}
fn default_trace_max_string_bytes() -> usize {
    4 * 1024
}
fn default_trace_shutdown_timeout_secs() -> u64 {
    10
}
fn default_http_slow_ms() -> u64 {
    1_000
}
fn default_query_slow_ms() -> u64 {
    5_000
}
fn default_ingest_slow_ms() -> u64 {
    2_000
}
fn default_database_slow_ms() -> u64 {
    200
}
fn default_object_store_slow_ms() -> u64 {
    500
}
fn default_external_slow_ms() -> u64 {
    1_000
}
fn default_background_slow_ms() -> u64 {
    30_000
}
fn default_otlp_protocol() -> String {
    "grpc".into()
}
fn default_export_timeout_ms() -> u64 {
    5_000
}
fn default_export_queue_capacity() -> usize {
    8_192
}
fn default_export_batch_size() -> usize {
    256
}
fn default_profiling_bind() -> String {
    "127.0.0.1".into()
}
fn default_profiling_port() -> u16 {
    5084
}
