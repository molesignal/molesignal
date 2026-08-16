// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Direct scanners for the narrow RUM physical datasets.

use std::{collections::HashSet, sync::Arc};

use arrow::array::RecordBatch;
use futures::{StreamExt, stream};
use object_store::ObjectStore;

use super::model::{
    ACTION_DETAIL_COLUMNS, ERROR_DETAIL_COLUMNS, RumActionRecord, RumErrorRecord, RumSessionRecord,
    SESSION_COLUMNS,
};
use crate::{
    domain::{
        storage::{ParquetFileMeta, ParquetFileMetaRepository, PhysicalDatasetKind},
        stream::StreamType,
    },
    infra::storage::parquet::reader::{ParquetReader, ReadOptions},
    shared::{
        Error, Result,
        ids::Id,
        time::{TimeRange, TimestampMicros},
    },
};

const FILE_READ_CONCURRENCY: usize = 16;

#[derive(Clone, Debug)]
pub(super) struct ScanFile {
    pub(super) meta: ParquetFileMeta,
    pub(super) range: TimeRange,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ScanStats {
    pub files: usize,
    pub rows: usize,
}

pub struct RumReadModelReader {
    pub(super) files: Arc<dyn ParquetFileMetaRepository>,
    pub(super) object_store: Arc<dyn ObjectStore>,
}

impl RumReadModelReader {
    pub fn new(
        files: Arc<dyn ParquetFileMetaRepository>,
        object_store: Arc<dyn ObjectStore>,
    ) -> Self {
        Self {
            files,
            object_store,
        }
    }

    pub async fn visit_sessions<F>(
        &self,
        org_id: &Id,
        range: TimeRange,
        columns: &[&str],
        mut visitor: F,
    ) -> Result<ScanStats>
    where
        F: FnMut(RumSessionRecord),
    {
        let files = self
            .source_files(
                org_id,
                "rum_sessions",
                PhysicalDatasetKind::RumSessionSummary,
                range,
            )
            .await?;
        self.visit_files(files, columns, |batch, scan_range| {
            for row in 0..batch.num_rows() {
                if let Some(record) = RumSessionRecord::from_batch(batch, row)
                    && in_range(record.timestamp_micros, scan_range)
                {
                    visitor(record);
                }
            }
        })
        .await
    }

    pub async fn visit_actions<F>(
        &self,
        org_id: &Id,
        range: TimeRange,
        columns: &[&str],
        mut visitor: F,
    ) -> Result<ScanStats>
    where
        F: FnMut(RumActionRecord),
    {
        let files = self
            .source_files(
                org_id,
                "rum_actions",
                PhysicalDatasetKind::RumActionSummary,
                range,
            )
            .await?;
        self.visit_files(files, columns, |batch, scan_range| {
            for row in 0..batch.num_rows() {
                if let Some(record) = RumActionRecord::from_batch(batch, row)
                    && in_range(record.timestamp_micros, scan_range)
                {
                    visitor(record);
                }
            }
        })
        .await
    }

    pub async fn visit_actions_for_sessions<F>(
        &self,
        org_id: &Id,
        range: TimeRange,
        session_ids: &HashSet<String>,
        columns: &[&str],
        mut visitor: F,
    ) -> Result<ScanStats>
    where
        F: FnMut(RumActionRecord),
    {
        if session_ids.is_empty() {
            return Ok(ScanStats::default());
        }
        let files = self
            .source_files(
                org_id,
                "rum_actions",
                PhysicalDatasetKind::RumActionSummary,
                range,
            )
            .await?
            .into_iter()
            .filter(|file| file_may_contain_session(&file.meta, session_ids))
            .collect();
        self.visit_files(files, columns, |batch, scan_range| {
            for row in 0..batch.num_rows() {
                if let Some(record) = RumActionRecord::from_batch(batch, row)
                    && in_range(record.timestamp_micros, scan_range)
                    && session_ids.contains(&record.session_id)
                {
                    visitor(record);
                }
            }
        })
        .await
    }

    pub async fn visit_raw_sessions_for_ids<F>(
        &self,
        org_id: &Id,
        range: TimeRange,
        session_ids: &HashSet<String>,
        mut visitor: F,
    ) -> Result<ScanStats>
    where
        F: FnMut(RumSessionRecord),
    {
        let files = self
            .raw_files(org_id, "rum_sessions", range)
            .await?
            .into_iter()
            .filter(|file| file_may_contain_session(&file.meta, session_ids))
            .collect();
        self.visit_files(files, SESSION_COLUMNS, |batch, scan_range| {
            for row in 0..batch.num_rows() {
                if let Some(record) = RumSessionRecord::from_batch(batch, row)
                    && in_range(record.timestamp_micros, scan_range)
                    && session_ids.contains(&record.session_id)
                {
                    visitor(record);
                }
            }
        })
        .await
    }

    pub async fn visit_raw_actions_for_sessions<F>(
        &self,
        org_id: &Id,
        range: TimeRange,
        session_ids: &HashSet<String>,
        mut visitor: F,
    ) -> Result<ScanStats>
    where
        F: FnMut(RumActionRecord),
    {
        let files = self
            .raw_files(org_id, "rum_actions", range)
            .await?
            .into_iter()
            .filter(|file| file_may_contain_session(&file.meta, session_ids))
            .collect();
        self.visit_files(files, ACTION_DETAIL_COLUMNS, |batch, scan_range| {
            for row in 0..batch.num_rows() {
                if let Some(record) = RumActionRecord::from_batch(batch, row)
                    && in_range(record.timestamp_micros, scan_range)
                    && session_ids.contains(&record.session_id)
                {
                    visitor(record);
                }
            }
        })
        .await
    }

    pub async fn visit_raw_errors_by_fingerprint<F>(
        &self,
        org_id: &Id,
        range: TimeRange,
        fingerprint: &str,
        mut visitor: F,
    ) -> Result<ScanStats>
    where
        F: FnMut(RumErrorRecord),
    {
        let files = self
            .raw_files(org_id, "rum_errors", range)
            .await?
            .into_iter()
            .filter(|file| file_may_contain_value(&file.meta, "fingerprint", fingerprint))
            .collect();
        self.visit_files(files, ERROR_DETAIL_COLUMNS, |batch, scan_range| {
            for row in 0..batch.num_rows() {
                if let Some(record) = RumErrorRecord::from_batch(batch, row)
                    && in_range(record.timestamp_micros, scan_range)
                    && record.fingerprint == fingerprint
                {
                    visitor(record);
                }
            }
        })
        .await
    }

    pub async fn visit_errors<F>(
        &self,
        org_id: &Id,
        range: TimeRange,
        columns: &[&str],
        mut visitor: F,
    ) -> Result<ScanStats>
    where
        F: FnMut(RumErrorRecord),
    {
        let files = self
            .source_files(
                org_id,
                "rum_errors",
                PhysicalDatasetKind::RumErrorSummary,
                range,
            )
            .await?;
        self.visit_files(files, columns, |batch, scan_range| {
            for row in 0..batch.num_rows() {
                if let Some(record) = RumErrorRecord::from_batch(batch, row)
                    && in_range(record.timestamp_micros, scan_range)
                {
                    visitor(record);
                }
            }
        })
        .await
    }

    pub(super) async fn source_files(
        &self,
        org_id: &Id,
        stream: &str,
        summary_kind: PhysicalDatasetKind,
        range: TimeRange,
    ) -> Result<Vec<ScanFile>> {
        let (summaries, raw) = tokio::try_join!(
            self.files
                .find_dataset(org_id, stream, StreamType::Logs, summary_kind, range,),
            self.files.find_dataset(
                org_id,
                stream,
                StreamType::Logs,
                PhysicalDatasetKind::Raw,
                range,
            ),
        )?;
        if summaries.is_empty() {
            return Ok(raw
                .into_iter()
                .map(|meta| ScanFile { meta, range })
                .collect());
        }

        // A deployment can introduce a new physical projection while older raw files still
        // exist. Read the raw prefix before the first summary row, then use summaries from that
        // exact timestamp onward. This keeps upgrades visible without permanently double-scanning.
        let summary_start = summaries
            .iter()
            .map(|file| file.time_range.start.0)
            .min()
            .unwrap_or(range.start.0);
        let mut files = summaries
            .into_iter()
            .map(|meta| ScanFile { meta, range })
            .collect::<Vec<_>>();
        if summary_start > range.start.0 {
            let prefix = TimeRange::new(range.start, TimestampMicros(summary_start));
            files.extend(
                raw.into_iter()
                    .filter(|file| file.time_range.start.0 < summary_start)
                    .map(|meta| ScanFile {
                        meta,
                        range: prefix,
                    }),
            );
        }
        Ok(files)
    }

    async fn raw_files(
        &self,
        org_id: &Id,
        stream: &str,
        range: TimeRange,
    ) -> Result<Vec<ScanFile>> {
        Ok(self
            .files
            .find_dataset(
                org_id,
                stream,
                StreamType::Logs,
                PhysicalDatasetKind::Raw,
                range,
            )
            .await?
            .into_iter()
            .map(|meta| ScanFile { meta, range })
            .collect())
    }

    async fn visit_files<F>(
        &self,
        files: Vec<ScanFile>,
        columns: &[&str],
        mut visitor: F,
    ) -> Result<ScanStats>
    where
        F: FnMut(&RecordBatch, TimeRange),
    {
        let reads = stream::iter(files).map(|file| {
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
        let mut stats = ScanStats::default();
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

fn in_range(timestamp_micros: i64, range: TimeRange) -> bool {
    timestamp_micros >= range.start.0 && timestamp_micros < range.end.0
}

fn file_may_contain_session(file: &ParquetFileMeta, session_ids: &HashSet<String>) -> bool {
    let Some(minimum) = file
        .min_values
        .get("session_id")
        .and_then(|value| value.as_str())
    else {
        return true;
    };
    let Some(maximum) = file
        .max_values
        .get("session_id")
        .and_then(|value| value.as_str())
    else {
        return true;
    };
    session_ids
        .iter()
        .any(|session_id| session_id.as_str() >= minimum && session_id.as_str() <= maximum)
}

fn file_may_contain_value(file: &ParquetFileMeta, field: &str, value: &str) -> bool {
    let Some(minimum) = file.min_values.get(field).and_then(|value| value.as_str()) else {
        return true;
    };
    let Some(maximum) = file.max_values.get(field).and_then(|value| value.as_str()) else {
        return true;
    };
    value >= minimum && value <= maximum
}
