// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Bounded exponential backoff with low-cost per-worker jitter.

use std::time::Duration;

/// Backoff for database-backed workers that must retain a fallback poll.
///
/// The uncapped delay doubles after every idle/error outcome. A ±20% jitter prevents all nodes
/// from acquiring metadata connections on the same clock boundary. Successful work should call
/// [`Self::reset`].
#[derive(Debug, Clone)]
pub struct PollingBackoff {
    base_millis: u64,
    max_millis: u64,
    attempts: u32,
    random_state: u64,
}

impl PollingBackoff {
    pub fn new(base: Duration, max: Duration, worker_key: &str) -> Self {
        let base_millis = duration_millis(base).max(1);
        let max_millis = duration_millis(max).max(base_millis);
        let mut random_state = 0xcbf2_9ce4_8422_2325_u64;
        for byte in worker_key.bytes() {
            random_state ^= u64::from(byte);
            random_state = random_state.wrapping_mul(0x1000_0000_01b3);
        }
        random_state ^= u64::from(std::process::id());
        random_state ^= std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;
        if random_state == 0 {
            random_state = 0x9e37_79b9_7f4a_7c15;
        }
        Self {
            base_millis,
            max_millis,
            attempts: 0,
            random_state,
        }
    }

    pub fn reset(&mut self) {
        self.attempts = 0;
    }

    pub fn next_delay(&mut self) -> Duration {
        let multiplier = 1_u64 << self.attempts.min(20);
        let capped = self
            .base_millis
            .saturating_mul(multiplier)
            .min(self.max_millis);
        self.attempts = self.attempts.saturating_add(1);

        let delta = (capped / 5).max(1);
        let lower = capped.saturating_sub(delta);
        let width = delta.saturating_mul(2).saturating_add(1);
        let jittered = lower.saturating_add(self.next_random() % width);
        Duration::from_millis(jittered.max(1))
    }

    fn next_random(&mut self) -> u64 {
        let mut value = self.random_state;
        value ^= value << 13;
        value ^= value >> 7;
        value ^= value << 17;
        self.random_state = value;
        value
    }
}

fn duration_millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delay_grows_to_cap_and_stays_jittered() {
        let mut backoff = PollingBackoff::new(
            Duration::from_millis(100),
            Duration::from_millis(800),
            "test-worker",
        );
        for (lower, upper) in [(80, 120), (160, 240), (320, 480), (640, 960), (640, 960)] {
            let delay = backoff.next_delay().as_millis();
            assert!((lower..=upper).contains(&delay));
        }
    }

    #[test]
    fn reset_returns_to_base_delay() {
        let mut backoff = PollingBackoff::new(
            Duration::from_millis(100),
            Duration::from_millis(800),
            "reset-worker",
        );
        let _ = backoff.next_delay();
        let _ = backoff.next_delay();
        backoff.reset();
        assert!((80..=120).contains(&backoff.next_delay().as_millis()));
    }
}
