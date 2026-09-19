// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Concurrent Parquet and immutable buffer generation visitation.

use arrow::array::RecordBatch;
use futures::{StreamExt, stream};

use super::{RumReadModelReader, ScanBatch, ScanFile, ScanInput, ScanStats};
use crate::{
    infra::{
        query::catalog_source::QueryDatasetSnapshot,
        storage::parquet::reader::{ParquetReader, ReadOptions},
    },
    shared::{Error, Result, time::TimeRange},
};

const FILE_READ_CONCURRENCY: usize = 16;

impl ScanInput {
    pub(super) fn from_catalog(dataset: Option<&QueryDatasetSnapshot>, range: TimeRange) -> Self {
        let Some(dataset) = dataset else {
            return Self::default();
        };
        Self {
            files: dataset
                .files
                .iter()
                .cloned()
                .map(|meta| ScanFile { meta, range })
                .collect(),
            batches: dataset
                .buffered_batches
                .iter()
                .cloned()
                .map(|batch| ScanBatch { batch, range })
                .collect(),
        }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.files.is_empty() && self.batches.is_empty()
    }
}

impl RumReadModelReader {
    pub(super) async fn visit_files<F>(
        &self,
        input: ScanInput,
        columns: &[&str],
        mut visitor: F,
    ) -> Result<ScanStats>
    where
        F: FnMut(&RecordBatch, TimeRange),
    {
        let mut stats = ScanStats::default();
        for buffered in input.batches {
            stats.rows += buffered.batch.num_rows();
            visitor(&buffered.batch, buffered.range);
        }

        let reads = stream::iter(input.files).map(|file| {
            let store = self.object_store.clone();
            async move {
                let reader = ParquetReader::new(store.clone());
                let options = ReadOptions::new()
                    .with_time_range(file.range.start.0, file.range.end.0)
                    .with_columns(columns)
                    .with_known_size(file.meta.size_bytes);
                let result = reader
                    .read_from_store(store, &file.meta.object_key, options)
                    .await;
                (file, result)
            }
        });
        let mut reads = reads.buffer_unordered(FILE_READ_CONCURRENCY);
        while let Some((file, result)) = reads.next().await {
            let batches = match result {
                Ok(batches) => batches,
                Err(Error::NotFound(_)) => {
                    tracing::warn!(
                        object_key = %file.meta.object_key,
                        "RUM read-model parquet is missing"
                    );
                    continue;
                }
                Err(error) => return Err(error),
            };
            stats.files += 1;
            for batch in batches {
                stats.rows += batch.num_rows();
                visitor(&batch, file.range);
            }
        }
        tracing::debug!(
            scanned_files = stats.files,
            scanned_rows = stats.rows,
            "RUM physical read-model scan completed"
        );
        Ok(stats)
    }
}
