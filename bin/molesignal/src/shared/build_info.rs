// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::sync::OnceLock;

pub const RELEASE_CHANNEL_ENV: &str = "RELEASE_CHANNEL";

/// 当前部署的交付成熟度。它是运行时元数据，不参与二进制构建。
pub fn release_channel() -> &'static str {
    static RELEASE_CHANNEL: OnceLock<&'static str> = OnceLock::new();
    RELEASE_CHANNEL.get_or_init(|| {
        std::env::var(RELEASE_CHANNEL_ENV)
            .ok()
            .as_deref()
            .and_then(normalize_release_channel)
            .unwrap_or("unknown")
    })
}

fn normalize_release_channel(value: &str) -> Option<&'static str> {
    match value.trim() {
        "alpha" => Some("alpha"),
        "beta" => Some("beta"),
        "rc" => Some("rc"),
        "stable" => Some("stable"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::normalize_release_channel;

    #[test]
    fn accepts_supported_release_channels() {
        for channel in ["alpha", "beta", "rc", "stable"] {
            assert_eq!(normalize_release_channel(channel), Some(channel));
        }
    }

    #[test]
    fn rejects_values_that_are_not_release_channels() {
        assert_eq!(normalize_release_channel("release"), None);
        assert_eq!(normalize_release_channel("production"), None);
        assert_eq!(normalize_release_channel(""), None);
    }
}
