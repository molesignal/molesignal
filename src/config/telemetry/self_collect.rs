// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde::{Deserialize, Serialize};

/// 服务自身遥测配置。
///
/// `enabled` 固定控制 Profiles 和 Trace 回灌；Metrics 可通过
/// `metrics_enabled` 单独关闭。Trace 捕获还受 `telemetry.trace` 控制。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelfCollectSettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_retention_days")]
    pub retention_days: u32,
    #[serde(default = "default_retention_days")]
    pub metrics_retention_days: u32,
    #[serde(default = "default_retention_days")]
    pub traces_retention_days: u32,
    #[serde(default = "default_retention_days")]
    pub profiles_retention_days: u32,
    #[serde(default = "default_true")]
    pub metrics_enabled: bool,
    #[serde(default = "default_metrics_interval_secs")]
    pub metrics_interval_secs: u64,
    #[serde(default = "default_queue_capacity")]
    pub queue_capacity: usize,
    #[serde(default = "default_batch_max_events")]
    pub batch_max_events: usize,
    #[serde(default = "default_batch_max_delay_ms")]
    pub batch_max_delay_ms: u64,
    #[serde(default = "default_flush_timeout_secs")]
    pub flush_timeout_secs: u64,
    #[serde(default = "default_profile_kinds")]
    pub profile_kinds: Vec<String>,
    #[serde(default = "default_profile_interval_secs")]
    pub profile_interval_secs: u64,
    #[serde(default = "default_profile_duration_secs")]
    pub profile_duration_secs: u64,
}

impl Default for SelfCollectSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            retention_days: default_retention_days(),
            metrics_retention_days: default_retention_days(),
            traces_retention_days: default_retention_days(),
            profiles_retention_days: default_retention_days(),
            metrics_enabled: true,
            metrics_interval_secs: default_metrics_interval_secs(),
            queue_capacity: default_queue_capacity(),
            batch_max_events: default_batch_max_events(),
            batch_max_delay_ms: default_batch_max_delay_ms(),
            flush_timeout_secs: default_flush_timeout_secs(),
            profile_kinds: default_profile_kinds(),
            profile_interval_secs: default_profile_interval_secs(),
            profile_duration_secs: default_profile_duration_secs(),
        }
    }
}

impl SelfCollectSettings {
    pub fn validate(&self) -> anyhow::Result<()> {
        if !(1..=3650).contains(&self.retention_days) {
            anyhow::bail!("telemetry.self_collect.retention_days must be between 1 and 3650");
        }
        for (signal, days) in [
            ("metrics", self.metrics_retention_days),
            ("traces", self.traces_retention_days),
            ("profiles", self.profiles_retention_days),
        ] {
            if !(1..=3650).contains(&days) {
                anyhow::bail!(
                    "telemetry.self_collect.{signal}_retention_days must be between 1 and 3650"
                );
            }
        }
        if self.enabled && self.metrics_enabled && self.metrics_interval_secs == 0 {
            anyhow::bail!("telemetry.self_collect.metrics_interval_secs must be greater than zero");
        }
        if self.queue_capacity == 0 {
            anyhow::bail!("telemetry.self_collect.queue_capacity must be greater than zero");
        }
        if self.batch_max_events == 0 {
            anyhow::bail!("telemetry.self_collect.batch_max_events must be greater than zero");
        }
        if self.batch_max_delay_ms == 0 {
            anyhow::bail!("telemetry.self_collect.batch_max_delay_ms must be greater than zero");
        }
        if self.flush_timeout_secs == 0 {
            anyhow::bail!("telemetry.self_collect.flush_timeout_secs must be greater than zero");
        }
        if self.enabled {
            self.validate_profiles()?;
        }
        Ok(())
    }

    pub fn retention_days_for(
        &self,
        signal: crate::shared::self_telemetry::SelfTelemetrySignal,
    ) -> u32 {
        match signal {
            crate::shared::self_telemetry::SelfTelemetrySignal::Metrics => {
                self.metrics_retention_days
            }
            crate::shared::self_telemetry::SelfTelemetrySignal::Traces => {
                self.traces_retention_days
            }
            crate::shared::self_telemetry::SelfTelemetrySignal::Profiles => {
                self.profiles_retention_days
            }
        }
    }

    fn validate_profiles(&self) -> anyhow::Result<()> {
        if self.profile_interval_secs == 0 {
            anyhow::bail!("telemetry.self_collect.profile_interval_secs must be greater than zero");
        }
        if !(1..=120).contains(&self.profile_duration_secs) {
            anyhow::bail!("telemetry.self_collect.profile_duration_secs must be between 1 and 120");
        }
        if self.profile_kinds.is_empty() {
            anyhow::bail!("telemetry.self_collect.profile_kinds must not be empty");
        }
        for kind in &self.profile_kinds {
            if !matches!(kind.as_str(), "cpu" | "heap") {
                anyhow::bail!(
                    "telemetry.self_collect.profile_kinds contains unsupported kind `{kind}`"
                );
            }
        }
        Ok(())
    }
}

const fn default_true() -> bool {
    true
}

const fn default_retention_days() -> u32 {
    7
}

const fn default_metrics_interval_secs() -> u64 {
    15
}

const fn default_queue_capacity() -> usize {
    8192
}

const fn default_batch_max_events() -> usize {
    256
}

const fn default_batch_max_delay_ms() -> u64 {
    1000
}

const fn default_flush_timeout_secs() -> u64 {
    5
}

fn default_profile_kinds() -> Vec<String> {
    vec!["cpu".into()]
}

const fn default_profile_interval_secs() -> u64 {
    600
}

const fn default_profile_duration_secs() -> u64 {
    10
}
