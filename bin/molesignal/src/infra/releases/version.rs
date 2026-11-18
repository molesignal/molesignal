// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

/// Four numeric components; legacy suffixes remain below the corresponding final release.
/// Suffix labels do not select a release stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct ProductVersion([u64; 4], bool, u64);

impl ProductVersion {
    pub(super) fn parse(value: &str) -> Option<Self> {
        let value = value.strip_prefix('v').unwrap_or(value);
        let mut parts = value.splitn(4, '.');
        let mut numbers = [0; 4];

        for number in &mut numbers[..3] {
            let part = parts.next()?;

            if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }

            *number = part.parse().ok()?;
        }

        let tail = parts.next()?;

        let end = tail
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(tail.len());

        numbers[3] = tail[..end].parse().ok()?;
        let suffix = &tail[end..];

        if suffix.is_empty() {
            return Some(Self(numbers, true, 0));
        }

        if !suffix.starts_with(['.', '-'])
            || suffix.len() == 1
            || !suffix[1..]
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
        {
            return None;
        }

        let sequence = suffix.rsplit(['.', '-']).next()?.parse().ok()?;
        Some(Self(numbers, false, sequence))
    }
}
