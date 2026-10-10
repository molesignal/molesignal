// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `[object_store]` —— 对象存储（本地目录、S3、Azure、GCS）后端、凭据与传输参数。

use serde::{Deserialize, Serialize};

/// `[object_store]`：对象存储后端、凭据与传输参数。
///
/// 拒绝未知键：写错的键名会让启动失败。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
    #[serde(default)]
    pub path_style: bool,
    /// 三层凭据来源优先级：env > credentials_file > inline TOML
    #[serde(default)]
    pub credentials_file: Option<std::path::PathBuf>,
    /// 写入对象达到该大小（MiB）时改用 multipart 上传。
    #[serde(default = "default_multipart_threshold_mb")]
    pub multipart_threshold_mb: u32,
    /// multipart 上传的单分片大小（MiB）。
    #[serde(default = "default_multipart_part_size_mb")]
    pub multipart_part_size_mb: u32,
    /// 范围读取达到该大小（MiB）时拆成多个块并发读取。
    #[serde(default = "default_range_threshold_mb")]
    pub range_threshold_mb: u32,
    /// 分段读取的单块大小（MiB）。
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
#[serde(deny_unknown_fields)]
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

/// S3 要求除最后一个分片外，每个分片至少 5 MiB。
const MIN_MULTIPART_PART_SIZE_MB: u32 = 5;

/// 单个分片或读取块的上限，避免一次请求占用过多内存。
const MAX_TRANSFER_UNIT_MB: u32 = 1024;

impl ObjectStoreSettings {
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.multipart_threshold_mb == 0 {
            anyhow::bail!("object_store.multipart_threshold_mb must be at least 1");
        }

        if !(MIN_MULTIPART_PART_SIZE_MB..=MAX_TRANSFER_UNIT_MB)
            .contains(&self.multipart_part_size_mb)
        {
            anyhow::bail!(
                "object_store.multipart_part_size_mb must be in \
                 {MIN_MULTIPART_PART_SIZE_MB}..={MAX_TRANSFER_UNIT_MB}"
            );
        }

        if self.range_threshold_mb == 0 {
            anyhow::bail!("object_store.range_threshold_mb must be at least 1");
        }

        if !(1..=MAX_TRANSFER_UNIT_MB).contains(&self.range_chunk_mb) {
            anyhow::bail!("object_store.range_chunk_mb must be in 1..={MAX_TRANSFER_UNIT_MB}");
        }

        Ok(())
    }
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
            path_style: false,
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
