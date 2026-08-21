// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use crate::domain::synthetics::{MonitorState, MultiLocationPolicy};

/// Aggregates stable per-Location states without treating missing evidence as healthy.
pub fn aggregate_locations(
    states: impl IntoIterator<Item = MonitorState>,
    expected_locations: usize,
    policy: &MultiLocationPolicy,
) -> MonitorState {
    if expected_locations == 0 {
        return MonitorState::Unknown;
    }
    let mut healthy = 0usize;
    let mut flaky = 0usize;
    let mut degraded = 0usize;
    let mut failing = 0usize;
    let mut unknown = 0usize;
    for state in states.into_iter().take(expected_locations) {
        match state {
            MonitorState::Healthy => healthy += 1,
            MonitorState::Flaky => flaky += 1,
            MonitorState::Degraded => degraded += 1,
            MonitorState::Failing => failing += 1,
            MonitorState::Unknown => unknown += 1,
        }
    }
    unknown += expected_locations.saturating_sub(healthy + flaky + degraded + failing + unknown);
    let threshold = match policy {
        MultiLocationPolicy::Any => 1,
        MultiLocationPolicy::Quorum { required } => {
            (*required as usize).clamp(1, expected_locations)
        }
        MultiLocationPolicy::Majority => expected_locations / 2 + 1,
        MultiLocationPolicy::All => expected_locations,
    };
    if failing >= threshold {
        return MonitorState::Failing;
    }
    if failing + degraded >= threshold {
        return MonitorState::Degraded;
    }
    if failing + degraded + flaky >= threshold {
        return MonitorState::Flaky;
    }
    // If the missing evidence could still change the threshold verdict, the state is unknown.
    if failing + unknown >= threshold
        || failing + degraded + unknown >= threshold
        || failing + degraded + flaky + unknown >= threshold
    {
        return MonitorState::Unknown;
    }
    if healthy > 0 {
        MonitorState::Healthy
    } else {
        MonitorState::Unknown
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn majority_requires_stable_evidence() {
        assert_eq!(
            aggregate_locations(
                [
                    MonitorState::Failing,
                    MonitorState::Failing,
                    MonitorState::Healthy,
                ],
                3,
                &MultiLocationPolicy::Majority,
            ),
            MonitorState::Failing
        );
        assert_eq!(
            aggregate_locations(
                [MonitorState::Healthy, MonitorState::Unknown],
                3,
                &MultiLocationPolicy::Majority,
            ),
            MonitorState::Unknown
        );
    }

    #[test]
    fn any_policy_never_hides_unknown() {
        assert_eq!(
            aggregate_locations(
                [MonitorState::Healthy, MonitorState::Unknown],
                2,
                &MultiLocationPolicy::Any,
            ),
            MonitorState::Unknown
        );
    }

    #[test]
    fn flaky_is_a_first_class_aggregate_state() {
        assert_eq!(
            aggregate_locations(
                [
                    MonitorState::Flaky,
                    MonitorState::Healthy,
                    MonitorState::Flaky
                ],
                3,
                &MultiLocationPolicy::Majority,
            ),
            MonitorState::Flaky
        );
        assert_eq!(
            aggregate_locations(
                [MonitorState::Degraded, MonitorState::Flaky],
                2,
                &MultiLocationPolicy::All,
            ),
            MonitorState::Flaky
        );
    }
}
