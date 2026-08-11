// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `[apm]` bounded projection, storage and query settings.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApmSettings {
    #[serde(default = "default_queue_capacity")]
    pub queue_capacity: usize,
    #[serde(default = "default_flush_interval_ms")]
    pub flush_interval_ms: u64,
    #[serde(default = "default_flush_max_snapshots")]
    pub flush_max_snapshots: usize,
    #[serde(default = "default_shutdown_drain_secs")]
    pub shutdown_drain_secs: u64,
    #[serde(default = "default_late_grace_secs")]
    pub late_grace_secs: u64,
    #[serde(default = "default_hot_retention_hours")]
    pub hot_retention_hours: u32,
    #[serde(default = "default_rollup_retention_days")]
    pub rollup_retention_days: u32,
    #[serde(default = "default_max_query_range_days")]
    pub max_query_range_days: u32,
    #[serde(default = "default_max_exemplars_per_bucket")]
    pub max_exemplars_per_bucket: usize,
    #[serde(default = "default_max_error_samples_per_group")]
    pub max_error_samples_per_group: usize,
    #[serde(default)]
    pub histogram: ApmHistogramSettings,
    #[serde(default)]
    pub cardinality: ApmCardinalitySettings,
    #[serde(default)]
    pub version_comparison: ApmVersionComparisonSettings,
}

impl Default for ApmSettings {
    fn default() -> Self {
        Self {
            queue_capacity: default_queue_capacity(),
            flush_interval_ms: default_flush_interval_ms(),
            flush_max_snapshots: default_flush_max_snapshots(),
            shutdown_drain_secs: default_shutdown_drain_secs(),
            late_grace_secs: default_late_grace_secs(),
            hot_retention_hours: default_hot_retention_hours(),
            rollup_retention_days: default_rollup_retention_days(),
            max_query_range_days: default_max_query_range_days(),
            max_exemplars_per_bucket: default_max_exemplars_per_bucket(),
            max_error_samples_per_group: default_max_error_samples_per_group(),
            histogram: ApmHistogramSettings::default(),
            cardinality: ApmCardinalitySettings::default(),
            version_comparison: ApmVersionComparisonSettings::default(),
        }
    }
}

impl ApmSettings {
    pub fn validate(&self) -> anyhow::Result<()> {
        if !(1_024..=1_048_576).contains(&self.queue_capacity) {
            anyhow::bail!("apm.queue_capacity must be between 1024 and 1048576");
        }
        if !(100..=60_000).contains(&self.flush_interval_ms) {
            anyhow::bail!("apm.flush_interval_ms must be between 100 and 60000");
        }
        if self.flush_max_snapshots == 0
            || self.flush_max_snapshots > 100_000
            || self.shutdown_drain_secs == 0
            || self.shutdown_drain_secs > 300
        {
            anyhow::bail!(
                "apm flush_max_snapshots and shutdown_drain_secs must be within bounded limits"
            );
        }
        if self.late_grace_secs == 0
            || self.late_grace_secs >= u64::from(self.hot_retention_hours) * 3_600
        {
            anyhow::bail!("apm.late_grace_secs must be non-zero and shorter than hot retention");
        }
        if !(1..=168).contains(&self.hot_retention_hours)
            || !(1..=3_650).contains(&self.rollup_retention_days)
            || self.max_query_range_days == 0
            || self.max_query_range_days > self.rollup_retention_days
        {
            anyhow::bail!("apm retention and max_query_range_days are inconsistent");
        }
        if !(1..=16).contains(&self.max_exemplars_per_bucket)
            || !(1..=64).contains(&self.max_error_samples_per_group)
        {
            anyhow::bail!("apm exemplar and error sample limits are out of bounds");
        }
        self.histogram.validate()?;
        self.cardinality.validate()?;
        self.version_comparison.validate()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApmHistogramSettings {
    #[serde(default = "default_histogram_schema_version")]
    pub schema_version: u16,
    /// Finite millisecond upper bounds. The projector always appends +Inf.
    #[serde(default = "default_histogram_boundaries_ms")]
    pub boundaries_ms: Vec<u64>,
}

impl Default for ApmHistogramSettings {
    fn default() -> Self {
        Self {
            schema_version: default_histogram_schema_version(),
            boundaries_ms: default_histogram_boundaries_ms(),
        }
    }
}

impl ApmHistogramSettings {
    fn validate(&self) -> anyhow::Result<()> {
        if self.schema_version == 0 || !(2..=64).contains(&self.boundaries_ms.len()) {
            anyhow::bail!("apm.histogram schema_version/bucket count is invalid");
        }
        if self.boundaries_ms.contains(&0)
            || self.boundaries_ms.windows(2).any(|pair| pair[0] >= pair[1])
        {
            anyhow::bail!("apm.histogram.boundaries_ms must be strictly increasing");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApmCardinalitySettings {
    #[serde(default = "default_service_limit")]
    pub services_per_org_hour: usize,
    #[serde(default = "default_transaction_limit")]
    pub transactions_per_service_hour: usize,
    #[serde(default = "default_dependency_limit")]
    pub dependencies_per_service_hour: usize,
    #[serde(default = "default_error_group_limit")]
    pub error_groups_per_service_hour: usize,
    #[serde(default = "default_version_limit")]
    pub versions_per_service_hour: usize,
    #[serde(default = "default_instance_limit")]
    pub instances_per_service_hour: usize,
}

impl Default for ApmCardinalitySettings {
    fn default() -> Self {
        Self {
            services_per_org_hour: default_service_limit(),
            transactions_per_service_hour: default_transaction_limit(),
            dependencies_per_service_hour: default_dependency_limit(),
            error_groups_per_service_hour: default_error_group_limit(),
            versions_per_service_hour: default_version_limit(),
            instances_per_service_hour: default_instance_limit(),
        }
    }
}

impl ApmCardinalitySettings {
    fn validate(&self) -> anyhow::Result<()> {
        let values = [
            self.services_per_org_hour,
            self.transactions_per_service_hour,
            self.dependencies_per_service_hour,
            self.error_groups_per_service_hour,
            self.versions_per_service_hour,
            self.instances_per_service_hour,
        ];
        if values.contains(&0) || values.iter().any(|value| *value > 100_000) {
            anyhow::bail!("apm.cardinality limits must be between 1 and 100000");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApmVersionComparisonSettings {
    #[serde(default = "default_min_requests_per_version")]
    pub min_requests_per_version: u64,
}

impl Default for ApmVersionComparisonSettings {
    fn default() -> Self {
        Self {
            min_requests_per_version: default_min_requests_per_version(),
        }
    }
}

impl ApmVersionComparisonSettings {
    fn validate(&self) -> anyhow::Result<()> {
        if !(1..=10_000_000).contains(&self.min_requests_per_version) {
            anyhow::bail!(
                "apm.version_comparison.min_requests_per_version must be between 1 and 10000000"
            );
        }
        Ok(())
    }
}

fn default_queue_capacity() -> usize {
    65_536
}
fn default_flush_interval_ms() -> u64 {
    5_000
}
fn default_flush_max_snapshots() -> usize {
    10_000
}
fn default_shutdown_drain_secs() -> u64 {
    10
}
fn default_late_grace_secs() -> u64 {
    300
}
fn default_hot_retention_hours() -> u32 {
    24
}
fn default_rollup_retention_days() -> u32 {
    30
}
fn default_max_query_range_days() -> u32 {
    30
}
fn default_max_exemplars_per_bucket() -> usize {
    3
}
fn default_max_error_samples_per_group() -> usize {
    8
}
fn default_histogram_schema_version() -> u16 {
    1
}
fn default_histogram_boundaries_ms() -> Vec<u64> {
    vec![
        1, 2, 4, 8, 16, 32, 64, 128, 256, 512, 1_000, 2_000, 4_000, 8_000, 16_000, 30_000, 60_000,
    ]
}
fn default_service_limit() -> usize {
    200
}
fn default_transaction_limit() -> usize {
    32
}
fn default_dependency_limit() -> usize {
    16
}
fn default_error_group_limit() -> usize {
    16
}
fn default_version_limit() -> usize {
    16
}
fn default_instance_limit() -> usize {
    256
}
fn default_min_requests_per_version() -> u64 {
    1_000
}
