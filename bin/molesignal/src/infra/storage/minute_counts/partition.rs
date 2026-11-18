// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The minute counts of the segments of a sealed partition, in one object beside its manifest: a
//! query over many sealed partitions reads one object for each instead of one for each segment.
//! A sealed partition does not change, and the next generation of its manifest gets its own
//! object.

use std::{collections::HashMap, sync::Arc};

use bytes::Bytes;
use kernel::{Error, Result};
use serde::{Deserialize, Serialize};

use super::{MinuteCounts, pack, unpack};

const MAGIC: &[u8; 4] = b"MSMP";

/// Minute counts by segment id.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PartitionCounts {
    segments: HashMap<String, Arc<MinuteCounts>>,
}

#[derive(Serialize)]
struct Packed<'a> {
    segments: Vec<(&'a str, &'a MinuteCounts)>,
}

#[derive(Deserialize)]
struct Unpacked {
    segments: Vec<(String, MinuteCounts)>,
}

impl PartitionCounts {
    pub fn insert(&mut self, segment: String, counts: Arc<MinuteCounts>) {
        self.segments.insert(segment, counts);
    }

    pub fn get(&self, segment: &str) -> Option<&Arc<MinuteCounts>> {
        self.segments.get(segment)
    }

    pub fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }

    /// Bytes the counts hold in memory.
    pub fn weight(&self) -> usize {
        self.segments
            .iter()
            .map(|(segment, counts)| segment.len() + counts.weight())
            .sum()
    }

    pub fn encode(&self) -> Result<Bytes> {
        let mut segments = self
            .segments
            .iter()
            .map(|(segment, counts)| (segment.as_str(), counts.as_ref()))
            .collect::<Vec<_>>();

        segments.sort_unstable_by_key(|(segment, _)| *segment);
        pack(MAGIC, &Packed { segments })
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let unpacked: Unpacked = unpack(MAGIC, bytes)?;
        let mut partition = Self::default();

        for (segment, counts) in unpacked.segments {
            if !counts.consistent() {
                return Err(Error::internal("partition minute counts object is invalid"));
            }

            partition.insert(segment, Arc::new(counts));
        }

        Ok(partition)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn counts(rows: u64) -> MinuteCounts {
        MinuteCounts {
            fields: vec!["level".into()],
            values: vec![vec!["ERROR".into()]],
            minutes: vec![7],
            ids: vec![vec![0]],
            rows: vec![rows],
        }
    }

    #[test]
    fn every_segment_reads_back_its_own_counts() {
        let mut partition = PartitionCounts::default();
        partition.insert("s1".into(), Arc::new(counts(3)));
        partition.insert("s2".into(), Arc::new(counts(5)));

        let decoded = PartitionCounts::decode(&partition.encode().unwrap()).unwrap();

        assert_eq!(decoded, partition);
        assert_eq!(decoded.get("s2").map(|counts| counts.total()), Some(5));
        assert_eq!(decoded.get("s3"), None);
    }

    #[test]
    fn a_segment_object_is_not_a_partition_object() {
        let bytes = counts(3).encode().unwrap();

        assert!(PartitionCounts::decode(&bytes).is_err());
        assert!(MinuteCounts::decode(&PartitionCounts::default().encode().unwrap()).is_err());
    }
}
