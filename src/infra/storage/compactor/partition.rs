// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::BTreeMap;

use crate::{
    domain::storage::{ParquetFileMeta, PhysicalDatasetKind, hour_start_micros},
    shared::{Error, Result},
};

type PartitionKey = (PhysicalDatasetKind, i64);

/// 小文件只在相同 `(dataset_kind, UTC hour)` 内贪心组批。
pub(super) fn build_groups(
    files: impl IntoIterator<Item = ParquetFileMeta>,
    target_bytes: u64,
) -> (Vec<Vec<ParquetFileMeta>>, Vec<ParquetFileMeta>) {
    let mut partitions: BTreeMap<PartitionKey, Vec<ParquetFileMeta>> = BTreeMap::new();
    let mut invalid = Vec::new();
    for file in files {
        let start_hour = hour_start_micros(file.time_range.start).0;
        let end_hour = hour_start_micros(file.time_range.end).0;
        if start_hour != end_hour {
            invalid.push(file);
            continue;
        }
        partitions
            .entry((file.dataset_kind, start_hour))
            .or_default()
            .push(file);
    }

    let mut groups = Vec::new();
    for files in partitions.values_mut() {
        files.sort_by(|left, right| {
            left.time_range
                .start
                .cmp(&right.time_range.start)
                .then_with(|| left.id.0.cmp(&right.id.0))
        });
        let mut current = Vec::new();
        let mut size = 0_u64;
        for file in files.drain(..) {
            if !current.is_empty() && size.saturating_add(file.size_bytes) > target_bytes {
                if current.len() >= 2 {
                    groups.push(std::mem::take(&mut current));
                } else {
                    current.clear();
                }
                size = 0;
            }
            size = size.saturating_add(file.size_bytes);
            current.push(file);
        }
        if current.len() >= 2 {
            groups.push(current);
        }
    }
    (groups, invalid)
}

pub(super) fn validate_group(group: &[ParquetFileMeta]) -> Result<PhysicalDatasetKind> {
    let first = group
        .first()
        .ok_or_else(|| Error::invalid("compactor group is empty"))?;
    let partition_hour = hour_start_micros(first.time_range.start);
    for file in group {
        if file.org_id != first.org_id
            || file.stream != first.stream
            || file.stream_type != first.stream_type
            || file.dataset_kind != first.dataset_kind
            || hour_start_micros(file.time_range.start) != partition_hour
            || hour_start_micros(file.time_range.end) != partition_hour
        {
            return Err(Error::invalid(
                "compactor refuses a group spanning dataset, stream, or UTC hour",
            ));
        }
    }
    Ok(first.dataset_kind)
}

#[cfg(test)]
mod tests {
    use serde_json::Map;

    use super::*;
    use crate::{
        domain::stream::StreamType,
        shared::{
            ids::Id,
            time::{TimeRange, TimestampMicros},
        },
    };

    fn file(id: &str, kind: PhysicalDatasetKind, start: i64, size: u64) -> ParquetFileMeta {
        ParquetFileMeta {
            id: Id::from_string(id),
            org_id: Id::from_string("org"),
            stream: "app".into(),
            stream_type: StreamType::Traces,
            dataset_kind: kind,
            object_key: format!("{id}.parquet"),
            time_range: TimeRange::new(TimestampMicros(start), TimestampMicros(start + 1)),
            rows: 1,
            size_bytes: size,
            min_values: Map::new(),
            max_values: Map::new(),
            deleted: false,
        }
    }

    #[test]
    fn never_groups_across_hour_or_dataset() {
        let files = vec![
            file("a", PhysicalDatasetKind::Raw, 1, 10),
            file("b", PhysicalDatasetKind::Raw, 2, 10),
            file("c", PhysicalDatasetKind::Raw, 3_600_000_001, 10),
            file("d", PhysicalDatasetKind::Raw, 3_600_000_002, 10),
            file("e", PhysicalDatasetKind::TraceSummary, 1, 10),
            file("f", PhysicalDatasetKind::TraceSummary, 2, 10),
        ];
        let (groups, invalid) = build_groups(files, 100);
        assert!(invalid.is_empty());
        assert_eq!(groups.len(), 3);
        assert!(groups.iter().all(|group| validate_group(group).is_ok()));
    }

    #[test]
    fn rejects_a_file_that_itself_crosses_an_hour() {
        let mut crossing = file("x", PhysicalDatasetKind::Raw, 3_599_999_999, 10);
        crossing.time_range.end = TimestampMicros(3_600_000_001);
        let (groups, invalid) = build_groups(vec![crossing], 100);
        assert!(groups.is_empty());
        assert_eq!(invalid.len(), 1);
    }
}
