// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `[store]` —— 元数据库（`store.meta`）与对象存储（`store.object`）凭据与底层参数。

use serde::{Deserialize, Serialize};

/// `[store]` 段：元数据库与对象存储统一容器。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StoreSettings {
    #[serde(default)]
    pub meta: MetaStoreSettings,
    #[serde(default)]
    pub object: ObjectStoreSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetaStoreSettings {
    #[serde(default = "default_meta_backend")]
    pub backend: String,
    #[serde(default = "default_meta_dsn")]
    pub dsn: String,
    #[serde(default = "default_meta_min_conns")]
    pub min_connections: u32,
    #[serde(default = "default_meta_conns")]
    pub max_connections: u32,
}

fn default_meta_backend() -> String {
    "sqlite".into()
}
fn default_meta_dsn() -> String {
    "sqlite://./data/meta.db?mode=rwc".into()
}
fn default_meta_min_conns() -> u32 {
    2
}
fn default_meta_conns() -> u32 {
    16
}

impl Default for MetaStoreSettings {
    fn default() -> Self {
        Self {
            backend: default_meta_backend(),
            dsn: default_meta_dsn(),
            min_connections: default_meta_min_conns(),
            max_connections: default_meta_conns(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectStoreSettings {
    #[serde(default = "default_object_backend")]
    pub backend: String,
    #[serde(default = "default_object_root")]
    pub root: String,
    #[serde(default)]
    pub bucket: String,
    #[serde(default)]
    pub region: String,
    #[serde(default)]
    pub endpoint: String,
    #[serde(default)]
    pub access_key: String,
    #[serde(default)]
    pub secret_key: String,
    /// 三层凭据来源优先级：env > credentials_file > inline TOML
    #[serde(default)]
    pub credentials_file: Option<std::path::PathBuf>,
    #[serde(default = "default_multipart_threshold_mb")]
    pub multipart_threshold_mb: u32,
    #[serde(default = "default_multipart_part_size_mb")]
    pub multipart_part_size_mb: u32,
    #[serde(default = "default_range_threshold_mb")]
    pub range_threshold_mb: u32,
    #[serde(default = "default_range_chunk_mb")]
    pub range_chunk_mb: u32,
    #[serde(default = "default_max_concurrency")]
    pub max_concurrency: u32,
    #[serde(default = "default_op_timeout_secs")]
    pub op_timeout_secs: u32,
    #[serde(default = "default_health_probe_interval_secs")]
    pub health_probe_interval_secs: u32,
    #[serde(default)]
    pub retry: RetrySettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrySettings {
    #[serde(default = "default_retry_max_attempts")]
    pub max_attempts: u32,
    #[serde(default = "default_retry_base_backoff_ms")]
    pub base_backoff_ms: u64,
    #[serde(default = "default_retry_max_backoff_ms")]
    pub max_backoff_ms: u64,
    #[serde(default = "default_retry_jitter_ratio")]
    pub jitter_ratio: f32,
}

impl Default for RetrySettings {
    fn default() -> Self {
        Self {
            max_attempts: default_retry_max_attempts(),
            base_backoff_ms: default_retry_base_backoff_ms(),
            max_backoff_ms: default_retry_max_backoff_ms(),
            jitter_ratio: default_retry_jitter_ratio(),
        }
    }
}

fn default_retry_max_attempts() -> u32 {
    4
}
fn default_retry_base_backoff_ms() -> u64 {
    100
}
fn default_retry_max_backoff_ms() -> u64 {
    5_000
}
fn default_retry_jitter_ratio() -> f32 {
    0.2
}

fn default_object_backend() -> String {
    "local".into()
}
fn default_object_root() -> String {
    "./data/objects".into()
}
fn default_multipart_threshold_mb() -> u32 {
    32
}
fn default_multipart_part_size_mb() -> u32 {
    8
}
fn default_range_threshold_mb() -> u32 {
    16
}
fn default_range_chunk_mb() -> u32 {
    8
}
fn default_max_concurrency() -> u32 {
    8
}
fn default_op_timeout_secs() -> u32 {
    30
}
fn default_health_probe_interval_secs() -> u32 {
    30
}

impl Default for ObjectStoreSettings {
    fn default() -> Self {
        Self {
            backend: default_object_backend(),
            root: default_object_root(),
            bucket: String::new(),
            region: String::new(),
            endpoint: String::new(),
            access_key: String::new(),
            secret_key: String::new(),
            credentials_file: None,
            multipart_threshold_mb: default_multipart_threshold_mb(),
            multipart_part_size_mb: default_multipart_part_size_mb(),
            range_threshold_mb: default_range_threshold_mb(),
            range_chunk_mb: default_range_chunk_mb(),
            max_concurrency: default_max_concurrency(),
            op_timeout_secs: default_op_timeout_secs(),
            health_probe_interval_secs: default_health_probe_interval_secs(),
            retry: RetrySettings::default(),
        }
    }
}
