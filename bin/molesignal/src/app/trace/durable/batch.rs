// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Logging a mixed batch of candidates: one record per (org, stream, force keep), the process's
//! own spans left out. The intake logs a request this way, and a sampler owner a producer's
//! batch.

use std::collections::BTreeMap;

use kernel::Result;
use signals::tail_sampling::{ForceKeep, TraceCandidate};

use super::{SpanOrigin, TraceCandidateLog};

impl TraceCandidateLog {
    /// Writes the tenant spans of `candidates`; returns each candidate's place in the log, `None`
    /// for the process's own spans. An error means nothing was acknowledged; records written
    /// before the failing one stay, unsettled, and are replayed if this node goes away.
    pub async fn append_candidates(
        &self,
        candidates: &[TraceCandidate],
    ) -> Result<Vec<Option<SpanOrigin>>> {
        let mut groups: BTreeMap<(&str, &str, u8), Vec<usize>> = BTreeMap::new();

        for (index, candidate) in candidates.iter().enumerate() {
            if let Some(stream) = &candidate.stream {
                groups
                    .entry((
                        candidate.org_id.as_str(),
                        stream.as_str(),
                        force_keep_rank(candidate.force_keep),
                    ))
                    .or_default()
                    .push(index);
            }
        }

        let mut origins = vec![None; candidates.len()];

        for ((org_id, stream, _), indices) in groups {
            let force_keep = candidates[indices[0]].force_keep;

            let spans: Vec<_> = indices
                .iter()
                .map(|&index| candidates[index].span.clone())
                .collect();

            let written = self.append(org_id, stream, force_keep, &spans).await?;

            for (index, origin) in indices.into_iter().zip(written) {
                origins[index] = Some(origin);
            }
        }

        Ok(origins)
    }
}

fn force_keep_rank(force_keep: ForceKeep) -> u8 {
    match force_keep {
        ForceKeep::None => 0,
        ForceKeep::TrustedInternal => 1,
        ForceKeep::DebugToken => 2,
    }
}
