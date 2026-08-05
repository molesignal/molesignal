// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use dashmap::DashMap;

use crate::{
    config::ParquetRotationSettings,
    infra::ingester::{
        BufferKey,
        metrics::{observe_adaptive_target, observe_parquet_ratio},
    },
};

#[derive(Debug, Clone, Copy)]
struct StreamFeedback {
    encoded_raw_ewma: f64,
    threshold_bytes: usize,
}

/// 用成功 Parquet 文件的 encoded/raw 比例，为每个 stream 平滑调整下一代 raw 阈值。
pub struct AdaptiveRotation {
    enabled: bool,
    target_bytes: usize,
    min_bytes: usize,
    max_bytes: usize,
    alpha: f64,
    feedback: DashMap<BufferKey, StreamFeedback>,
}

impl AdaptiveRotation {
    pub fn new(settings: &ParquetRotationSettings, max_bytes: usize) -> Self {
        Self {
            enabled: settings.adaptive_enabled,
            target_bytes: mb_to_bytes(settings.target_file_size_mb),
            min_bytes: mb_to_bytes(settings.min_buffer_mb).min(max_bytes),
            max_bytes,
            alpha: settings.ewma_alpha,
            feedback: DashMap::new(),
        }
    }

    pub fn threshold_for(&self, key: &BufferKey) -> usize {
        if !self.enabled {
            return self.max_bytes;
        }
        self.feedback
            .get(key)
            .map(|feedback| feedback.threshold_bytes)
            .unwrap_or(self.max_bytes)
    }

    /// 只应在 Parquet 对象与 ParquetFileMeta 均提交成功后调用。
    pub fn observe(&self, key: &BufferKey, estimated_raw_bytes: usize, encoded_bytes: u64) {
        if estimated_raw_bytes == 0 {
            return;
        }
        let observed_ratio =
            ((encoded_bytes as f64) / (estimated_raw_bytes as f64)).clamp(1.0e-6, 1.0e6);
        observe_parquet_ratio(key.1.as_str(), observed_ratio);

        if !self.enabled {
            observe_adaptive_target(key.1.as_str(), self.max_bytes as f64);
            return;
        }
        let mut feedback = self.feedback.entry(key.clone()).or_insert(StreamFeedback {
            encoded_raw_ewma: observed_ratio,
            threshold_bytes: self.max_bytes,
        });
        if feedback.encoded_raw_ewma != observed_ratio {
            feedback.encoded_raw_ewma =
                self.alpha * observed_ratio + (1.0 - self.alpha) * feedback.encoded_raw_ewma;
        }
        let desired = (self.target_bytes as f64 / feedback.encoded_raw_ewma)
            .round()
            .clamp(self.min_bytes as f64, self.max_bytes as f64);
        feedback.threshold_bytes = desired as usize;
        observe_adaptive_target(key.1.as_str(), desired);
    }
}

fn mb_to_bytes(value: u32) -> usize {
    (value as usize).saturating_mul(1024 * 1024)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::{storage::PhysicalDatasetKind, stream::StreamType},
        shared::ids::Id,
    };

    fn key() -> BufferKey {
        (
            Id::from_string("org-a"),
            StreamType::Logs,
            "app".to_string(),
            PhysicalDatasetKind::Raw,
        )
    }

    fn settings(enabled: bool, alpha: f64) -> ParquetRotationSettings {
        ParquetRotationSettings {
            adaptive_enabled: enabled,
            target_file_size_mb: 50,
            min_buffer_mb: 10,
            ewma_alpha: alpha,
        }
    }

    #[test]
    fn starts_at_max_then_tracks_feedback_with_ewma() {
        let max = 100 * 1024 * 1024;
        let adaptive = AdaptiveRotation::new(&settings(true, 0.5), max);
        let key = key();
        assert_eq!(adaptive.threshold_for(&key), max);

        adaptive.observe(&key, 100 * 1024 * 1024, 100 * 1024 * 1024);
        assert_eq!(adaptive.threshold_for(&key), 50 * 1024 * 1024);

        adaptive.observe(&key, 100 * 1024 * 1024, 10 * 1024 * 1024);
        let threshold = adaptive.threshold_for(&key);
        assert!(threshold > 90 * 1024 * 1024);
        assert!(threshold < max);
    }

    #[test]
    fn clamps_to_min_and_max() {
        let max = 100 * 1024 * 1024;
        let adaptive = AdaptiveRotation::new(&settings(true, 1.0), max);
        let key = key();
        adaptive.observe(&key, 100 * 1024 * 1024, 1);
        assert_eq!(adaptive.threshold_for(&key), max);
        adaptive.observe(&key, 1, 100 * 1024 * 1024);
        assert_eq!(adaptive.threshold_for(&key), 10 * 1024 * 1024);
    }

    #[test]
    fn disabled_mode_keeps_hard_maximum() {
        let max = 100 * 1024 * 1024;
        let adaptive = AdaptiveRotation::new(&settings(false, 1.0), max);
        let key = key();
        adaptive.observe(&key, max, max as u64);
        assert_eq!(adaptive.threshold_for(&key), max);
    }
}
