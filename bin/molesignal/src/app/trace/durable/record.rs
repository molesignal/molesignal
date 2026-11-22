// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The records of a candidate log object: a request's spans, or a batch of settled-span markers.
//! Each record is JSON, framed with the dataset WAL's object format (checksum, optional seal).

use serde::{Deserialize, Serialize};
use signals::{
    tail_sampling::{ForceKeep, TraceCandidate},
    trace_normalization::CanonicalSpan,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub(super) enum LogRecord {
    /// Spans of one tenant stream acknowledged together.
    #[serde(rename_all = "camelCase")]
    Candidates {
        org_id: String,
        stream: String,
        force_keep: u8,
        spans: Vec<CanonicalSpan>,
    },
    /// Spans settled since the last marker, as `(sequence, record, span)`.
    Settled { spans: Vec<(u64, u32, u32)> },
}

impl LogRecord {
    pub(super) fn encode(&self) -> anyhow::Result<Vec<u8>> {
        Ok(serde_json::to_vec(self)?)
    }

    pub(super) fn decode(bytes: &[u8]) -> anyhow::Result<Self> {
        Ok(serde_json::from_slice(bytes)?)
    }

    /// The candidates a `Candidates` record holds, in span order.
    pub(super) fn candidates(&self) -> Vec<TraceCandidate> {
        let Self::Candidates {
            org_id,
            stream,
            force_keep,
            spans,
        } = self
        else {
            return Vec::new();
        };

        spans
            .iter()
            .map(|span| TraceCandidate {
                org_id: org_id.clone(),
                stream: Some(stream.clone()),
                span: span.clone(),
                force_keep: force_keep_of(*force_keep),
            })
            .collect()
    }
}

pub(super) fn force_keep_code(force_keep: ForceKeep) -> u8 {
    match force_keep {
        ForceKeep::None => 0,
        ForceKeep::TrustedInternal => 1,
        ForceKeep::DebugToken => 2,
    }
}

fn force_keep_of(code: u8) -> ForceKeep {
    match code {
        1 => ForceKeep::TrustedInternal,
        2 => ForceKeep::DebugToken,
        _ => ForceKeep::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_round_trip() {
        let span = signals::trace_fixtures::canonical_http_trace().remove(0);

        let candidates = LogRecord::Candidates {
            org_id: "org".into(),
            stream: "default".into(),
            force_keep: force_keep_code(ForceKeep::DebugToken),
            spans: vec![span.clone()],
        };

        let decoded = LogRecord::decode(&candidates.encode().unwrap()).unwrap();
        assert_eq!(decoded, candidates);

        let restored = decoded.candidates();
        assert_eq!(restored.len(), 1);
        assert_eq!(restored[0].span, span);
        assert_eq!(restored[0].stream.as_deref(), Some("default"));
        assert_eq!(restored[0].force_keep, ForceKeep::DebugToken);

        let settled = LogRecord::Settled {
            spans: vec![(3, 0, 1)],
        };

        assert_eq!(
            LogRecord::decode(&settled.encode().unwrap()).unwrap(),
            settled
        );
        assert!(settled.candidates().is_empty());
    }
}
