// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `[intake]` 与 `[compactor]` —— 写入缓冲 flush 与后台压缩/保留/降采样。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntakeSettings {
    #[serde(default = "default_buffer_max")]
    pub buffer_max_size: u32,
    #[serde(default = "default_max_buffer_memory")]
    pub max_buffer_memory_size: u32,
    #[serde(default = "default_flush_interval")]
    pub flush_interval_secs: u32,
    #[serde(default = "default_flush_parallel")]
    pub flush_parallelism: u32,
    #[serde(default)]
    pub rotation: ParquetRotationSettings,
    #[serde(default)]
    pub prometheus: PrometheusIntakeSettings,
}

fn default_buffer_max() -> u32 {
    256
}

fn default_max_buffer_memory() -> u32 {
    1024
}

fn default_flush_interval() -> u32 {
    30
}

fn default_flush_parallel() -> u32 {
    4
}

impl Default for IntakeSettings {
    fn default() -> Self {
        Self {
            buffer_max_size: default_buffer_max(),
            max_buffer_memory_size: default_max_buffer_memory(),
            flush_interval_secs: default_flush_interval(),
            flush_parallelism: default_flush_parallel(),
            rotation: ParquetRotationSettings::default(),
            prometheus: PrometheusIntakeSettings::default(),
        }
    }
}

impl IntakeSettings {
    pub fn validate(&self) -> anyhow::Result<()> {
        for (name, value) in [
            ("buffer_max_size", self.buffer_max_size),
            ("max_buffer_memory_size", self.max_buffer_memory_size),
            ("flush_interval_secs", self.flush_interval_secs),
            ("flush_parallelism", self.flush_parallelism),
        ] {
            if value == 0 {
                anyhow::bail!("intake.{name} must be greater than zero");
            }
        }

        self.rotation.validate(self.buffer_max_size)?;
        self.prometheus.validate()
    }
}

/// Parquet encoded-size feedback used to tune each stream's raw buffer threshold.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParquetRotationSettings {
    #[serde(default = "default_true")]
    pub adaptive_enabled: bool,
    #[serde(default = "default_target_file_size")]
    pub target_file_size: u32,
    #[serde(default = "default_min_buffer")]
    pub min_buffer_size: u32,
    #[serde(default = "default_rotation_alpha")]
    pub ewma_alpha: f64,
}

fn default_true() -> bool {
    true
}

fn default_target_file_size() -> u32 {
    128
}

fn default_min_buffer() -> u32 {
    16
}

fn default_rotation_alpha() -> f64 {
    0.2
}

impl Default for ParquetRotationSettings {
    fn default() -> Self {
        Self {
            adaptive_enabled: default_true(),
            target_file_size: default_target_file_size(),
            min_buffer_size: default_min_buffer(),
            ewma_alpha: default_rotation_alpha(),
        }
    }
}

impl ParquetRotationSettings {
    fn validate(&self, max_buffer_size: u32) -> anyhow::Result<()> {
        if self.target_file_size == 0 {
            anyhow::bail!("intake.rotation.target_file_size must be greater than zero");
        }

        if self.min_buffer_size == 0 || self.min_buffer_size > max_buffer_size {
            anyhow::bail!("intake.rotation.min_buffer_size must be in 1..=intake.buffer_max_size");
        }

        if !self.ewma_alpha.is_finite() || self.ewma_alpha <= 0.0 || self.ewma_alpha > 1.0 {
            anyhow::bail!("intake.rotation.ewma_alpha must be finite and in (0, 1]");
        }

        Ok(())
    }
}

/// Prometheus remote-write 在 schema/WAL 之前执行的结构保护。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrometheusIntakeSettings {
    /// 单个 series 除 `__name__` 外最多允许的 label 数。
    #[serde(default = "default_prometheus_max_labels")]
    pub max_labels_per_series: usize,
    /// label name 的最大 UTF-8 字节数。
    #[serde(default = "default_prometheus_max_label_name_length")]
    pub max_label_name_length: usize,
    /// label value 的最大 UTF-8 字节数。
    #[serde(default = "default_prometheus_max_label_value_length")]
    pub max_label_value_length: usize,
    /// 一个内部 IntakeBatch / WAL seq 最多承载的 sample 数。
    #[serde(default = "default_prometheus_max_samples_per_batch")]
    pub max_samples_per_batch: usize,
    #[serde(default)]
    pub cardinality: PrometheusCardinalitySettings,
}

fn default_prometheus_max_labels() -> usize {
    64
}

fn default_prometheus_max_label_name_length() -> usize {
    128
}

fn default_prometheus_max_label_value_length() -> usize {
    2048
}

fn default_prometheus_max_samples_per_batch() -> usize {
    16_384
}

impl Default for PrometheusIntakeSettings {
    fn default() -> Self {
        Self {
            max_labels_per_series: default_prometheus_max_labels(),
            max_label_name_length: default_prometheus_max_label_name_length(),
            max_label_value_length: default_prometheus_max_label_value_length(),
            max_samples_per_batch: default_prometheus_max_samples_per_batch(),
            cardinality: PrometheusCardinalitySettings::default(),
        }
    }
}

impl PrometheusIntakeSettings {
    fn validate(&self) -> anyhow::Result<()> {
        for (name, value) in [
            ("max_labels_per_series", self.max_labels_per_series),
            ("max_label_name_length", self.max_label_name_length),
            ("max_label_value_length", self.max_label_value_length),
            ("max_samples_per_batch", self.max_samples_per_batch),
        ] {
            if value == 0 {
                anyhow::bail!("intake.prometheus.{name} must be greater than zero");
            }
        }

        self.cardinality.validate()
    }
}

/// 当前 intake owner 上的 Prometheus active-series 准入。这里是进程的上限；组织与指标的上限是
/// 平台设置，运行中修改。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrometheusCardinalitySettings {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_max_active_series_process")]
    pub max_active_series_per_process: usize,
    #[serde(default = "default_max_new_series_minute")]
    pub max_new_series_per_minute: usize,
    #[serde(default = "default_series_idle_ttl")]
    pub idle_ttl_secs: u64,
}

fn default_max_active_series_process() -> usize {
    1_000_000
}

fn default_max_new_series_minute() -> usize {
    20_000
}

fn default_series_idle_ttl() -> u64 {
    900
}

const MAX_SERIES_IDLE_TTL_SECS: u64 = 365 * 24 * 60 * 60;

impl Default for PrometheusCardinalitySettings {
    fn default() -> Self {
        Self {
            enabled: default_true(),
            max_active_series_per_process: default_max_active_series_process(),
            max_new_series_per_minute: default_max_new_series_minute(),
            idle_ttl_secs: default_series_idle_ttl(),
        }
    }
}

impl PrometheusCardinalitySettings {
    fn validate(&self) -> anyhow::Result<()> {
        if !self.enabled {
            return Ok(());
        }

        for (name, value) in [
            (
                "max_active_series_per_process",
                self.max_active_series_per_process,
            ),
            ("max_new_series_per_minute", self.max_new_series_per_minute),
        ] {
            if value == 0 {
                anyhow::bail!("intake.prometheus.cardinality.{name} must be greater than zero");
            }
        }

        if self.idle_ttl_secs == 0 || self.idle_ttl_secs > MAX_SERIES_IDLE_TTL_SECS {
            anyhow::bail!(
                "intake.prometheus.cardinality.idle_ttl_secs must be in 1..={MAX_SERIES_IDLE_TTL_SECS}"
            );
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactorSettings {
    #[serde(default = "default_compact_interval")]
    pub interval_secs: u32,
    /// 一次合并的输入文件总大小上限，单位 MiB。
    #[serde(default = "default_compact_size")]
    pub target_size: u32,
    #[serde(default = "default_compact_concurrency")]
    pub max_concurrent_groups: u32,
}

fn default_compact_interval() -> u32 {
    300
}

fn default_compact_size() -> u32 {
    512
}

fn default_compact_concurrency() -> u32 {
    4
}

impl Default for CompactorSettings {
    fn default() -> Self {
        Self {
            interval_secs: default_compact_interval(),
            target_size: default_compact_size(),
            max_concurrent_groups: default_compact_concurrency(),
        }
    }
}

/// Largest `[compactor].target_size`, in MiB. A merge streams its inputs and its output, so the
/// limit is not memory: it keeps one merged file within 4096 parts of a multipart upload and a
/// merge within minutes.
const MAX_COMPACT_TARGET_SIZE: u32 = 64 * 1024;

impl CompactorSettings {
    pub fn target_bytes(&self) -> u64 {
        u64::from(self.target_size).saturating_mul(1024 * 1024)
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        if !(1..=MAX_COMPACT_TARGET_SIZE).contains(&self.target_size) {
            anyhow::bail!("compactor.target_size must be in 1..={MAX_COMPACT_TARGET_SIZE} (MiB)");
        }

        Ok(())
    }
}
