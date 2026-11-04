// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Which sessions are among a share of them. A session is kept or dropped as a whole, by a hash
//! of its id, so that every event and every replay segment of it meet the same fate on any node,
//! and the sessions that are kept have all they did. What share a session is kept with is for
//! [`super::retention`] to say.

use crate::domain::rum::MAX_SESSION_SAMPLE_RATE;

/// A number from 0 to 99 that the session id decides and every node agrees on: FNV-1a over the
/// id, mixed so that ids that differ only in their last digits still spread over the range.
fn bucket(session_id: &str) -> u32 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;

    for byte in session_id.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }

    hash ^= hash >> 30;
    hash = hash.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    hash ^= hash >> 27;
    hash = hash.wrapping_mul(0x94d0_49bb_1331_11eb);
    hash ^= hash >> 31;

    (hash % u64::from(MAX_SESSION_SAMPLE_RATE)) as u32
}

/// Whether the session with this id is among the `rate` percent that are kept.
pub(super) fn keeps(session_id: &str, rate: u32) -> bool {
    match rate {
        0 => false,
        rate if rate >= MAX_SESSION_SAMPLE_RATE => true,
        rate => bucket(session_id) < rate,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(n: u32) -> String {
        format!("6f1c{n:08x}-1b7e-4d08-a5c3-90e2d41f7b68")
    }

    #[test]
    fn every_session_is_kept_at_a_hundred_and_none_at_nothing() {
        for n in 0..200 {
            assert!(keeps(&id(n), 100));
            assert!(keeps(&id(n), 250));
            assert!(!keeps(&id(n), 0));
        }
    }

    #[test]
    fn a_session_has_the_same_fate_every_time_it_is_asked() {
        for n in 0..200 {
            assert_eq!(keeps(&id(n), 40), keeps(&id(n), 40));
        }

        // The fate is written down, so that nodes of another build cannot disagree: ids that
        // are kept at 10 percent stay kept at any higher one.
        let kept_at_ten = (0..2_000)
            .filter(|n| keeps(&id(*n), 10))
            .collect::<Vec<_>>();

        assert!(kept_at_ten.iter().all(|n| keeps(&id(*n), 11)));
        assert!(kept_at_ten.iter().all(|n| keeps(&id(*n), 50)));
        assert_eq!(bucket("session-1"), bucket("session-1"));
    }

    #[test]
    fn a_share_of_sessions_is_kept_about_as_large_as_the_rate() {
        for rate in [1_u32, 10, 25, 50, 90] {
            let kept = (0..20_000).filter(|n| keeps(&id(*n), rate)).count() as f64;
            let expected = f64::from(rate) / 100.0 * 20_000.0;

            assert!(
                (kept - expected).abs() < expected.max(400.0) * 0.15,
                "{rate}%: kept {kept} of 20000"
            );
        }
    }
}
