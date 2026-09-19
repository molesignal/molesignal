// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Direct scanners for the narrow RUM physical datasets.

mod source;

use std::{collections::HashSet, sync::Arc};

use arrow::array::{Array, RecordBatch, TimestampMicrosecondArray};
use object_store::ObjectStore;

use super::model::{
    ACTION_DETAIL_COLUMNS, ERROR_DETAIL_COLUMNS, RumActionRecord, RumErrorRecord, RumSessionRecord,
    SESSION_COLUMNS,
};
use crate::{
    domain::{
        storage::{
            DatasetTypeId, QueryFile, QueryFileSource, primary_dataset_type, type_id::builtin,
        },
        stream::StreamType,
    },
    infra::{query::catalog_source::CatalogQuerySource, storage::arrow_schema::TS_COL},
    shared::{
        Result,
        ids::Id,
        time::{TimeRange, TimestampMicros},
    },
};

#[derive(Clone, Debug)]
pub(super) struct ScanFile {
    pub(super) meta: QueryFile,
    pub(super) range: TimeRange,
}

#[derive(Clone, Debug)]
pub(in crate::infra::rum::read_model) struct ScanBatch {
    pub(in crate::infra::rum::read_model) batch: RecordBatch,
    pub(in crate::infra::rum::read_model) range: TimeRange,
}

#[derive(Clone, Debug, Default)]
pub(super) struct ScanInput {
    pub(super) files: Vec<ScanFile>,
    pub(in crate::infra::rum::read_model) batches: Vec<ScanBatch>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ScanStats {
    pub files: usize,
    pub rows: usize,
}

pub struct RumReadModelReader {
    pub(super) files: Arc<dyn QueryFileSource>,
    pub(super) object_store: Arc<dyn ObjectStore>,
    catalog_source: Option<Arc<CatalogQuerySource>>,
}

impl RumReadModelReader {
    pub fn new(files: Arc<dyn QueryFileSource>, object_store: Arc<dyn ObjectStore>) -> Self {
        Self {
            files,
            object_store,
            catalog_source: None,
        }
    }

    pub fn with_catalog_source(mut self, source: Arc<CatalogQuerySource>) -> Self {
        self.catalog_source = Some(source);
        self
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
        let input = self
            .source_files(
                org_id,
                "rum_sessions",
                DatasetTypeId::builtin(builtin::DATASET_RUM_SESSION_SUMMARY),
                range,
            )
            .await?;
        self.visit_files(input, columns, |batch, scan_range| {
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
        let input = self
            .source_files(
                org_id,
                "rum_actions",
                DatasetTypeId::builtin(builtin::DATASET_RUM_ACTION_SUMMARY),
                range,
            )
            .await?;
        self.visit_files(input, columns, |batch, scan_range| {
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
        let mut input = self
            .source_files(
                org_id,
                "rum_actions",
                DatasetTypeId::builtin(builtin::DATASET_RUM_ACTION_SUMMARY),
                range,
            )
            .await?;
        input
            .files
            .retain(|file| file_may_contain_session(&file.meta, session_ids));
        self.visit_files(input, columns, |batch, scan_range| {
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
        let mut input = self.raw_files(org_id, "rum_sessions", range).await?;
        input
            .files
            .retain(|file| file_may_contain_session(&file.meta, session_ids));
        self.visit_files(input, SESSION_COLUMNS, |batch, scan_range| {
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
        let mut input = self.raw_files(org_id, "rum_actions", range).await?;
        input
            .files
            .retain(|file| file_may_contain_session(&file.meta, session_ids));
        self.visit_files(input, ACTION_DETAIL_COLUMNS, |batch, scan_range| {
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
        let mut input = self.raw_files(org_id, "rum_errors", range).await?;
        input
            .files
            .retain(|file| file_may_contain_value(&file.meta, "fingerprint", fingerprint));
        self.visit_files(input, ERROR_DETAIL_COLUMNS, |batch, scan_range| {
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
        let input = self
            .source_files(
                org_id,
                "rum_errors",
                DatasetTypeId::builtin(builtin::DATASET_RUM_ERROR_SUMMARY),
                range,
            )
            .await?;
        self.visit_files(input, columns, |batch, scan_range| {
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
        summary_type: DatasetTypeId,
        range: TimeRange,
    ) -> Result<ScanInput> {
        let raw_type = primary_dataset_type(StreamType::LOGS)?;
        let (mut summaries, mut raw) = match &self.catalog_source {
            Some(source) => {
                let snapshot = source
                    .snapshot_by_name(
                        org_id,
                        stream,
                        StreamType::LOGS,
                        &[summary_type.clone(), raw_type.clone()],
                        range,
                    )
                    .await?;
                (
                    ScanInput::from_catalog(snapshot.dataset(&summary_type), range),
                    ScanInput::from_catalog(snapshot.dataset(&raw_type), range),
                )
            }
            None => {
                let (summaries, raw) = tokio::try_join!(
                    self.files
                        .find_dataset(org_id, stream, StreamType::LOGS, summary_type, range,),
                    self.files
                        .find_dataset(org_id, stream, StreamType::LOGS, raw_type, range,),
                )?;
                (
                    ScanInput {
                        files: summaries
                            .into_iter()
                            .map(|meta| ScanFile { meta, range })
                            .collect(),
                        batches: Vec::new(),
                    },
                    ScanInput {
                        files: raw
                            .into_iter()
                            .map(|meta| ScanFile { meta, range })
                            .collect(),
                        batches: Vec::new(),
                    },
                )
            }
        };
        if summaries.is_empty() {
            return Ok(raw);
        }

        // A deployment can introduce a new physical projection while older raw files still
        // exist. Read the raw prefix before the first summary row, then use summaries from that
        // exact timestamp onward. This keeps upgrades visible without permanently double-scanning.
        let summary_start = summaries
            .files
            .iter()
            .map(|file| file.meta.time_range.start.0)
            .chain(
                summaries
                    .batches
                    .iter()
                    .filter_map(|batch| batch_min_timestamp(&batch.batch)),
            )
            .min()
            .unwrap_or(range.start.0);
        if summary_start > range.start.0 {
            let prefix = TimeRange::new(range.start, TimestampMicros(summary_start));
            summaries
                .files
                .extend(raw.files.drain(..).filter_map(|mut file| {
                    (file.meta.time_range.start.0 < summary_start).then(|| {
                        file.range = prefix;
                        file
                    })
                }));
            summaries
                .batches
                .extend(raw.batches.drain(..).map(|mut batch| {
                    batch.range = prefix;
                    batch
                }));
        }
        Ok(summaries)
    }

    async fn raw_files(&self, org_id: &Id, stream: &str, range: TimeRange) -> Result<ScanInput> {
        let raw_type = primary_dataset_type(StreamType::LOGS)?;
        match &self.catalog_source {
            Some(source) => {
                let snapshot = source
                    .snapshot_by_name(
                        org_id,
                        stream,
                        StreamType::LOGS,
                        std::slice::from_ref(&raw_type),
                        range,
                    )
                    .await?;
                Ok(ScanInput::from_catalog(snapshot.dataset(&raw_type), range))
            }
            None => Ok(ScanInput {
                files: self
                    .files
                    .find_dataset(org_id, stream, StreamType::LOGS, raw_type, range)
                    .await?
                    .into_iter()
                    .map(|meta| ScanFile { meta, range })
                    .collect(),
                batches: Vec::new(),
            }),
        }
    }
}

fn batch_min_timestamp(batch: &RecordBatch) -> Option<i64> {
    batch
        .column_by_name(TS_COL)?
        .as_any()
        .downcast_ref::<TimestampMicrosecondArray>()?
        .iter()
        .flatten()
        .min()
}

fn in_range(timestamp_micros: i64, range: TimeRange) -> bool {
    timestamp_micros >= range.start.0 && timestamp_micros < range.end.0
}

fn file_may_contain_session(file: &QueryFile, session_ids: &HashSet<String>) -> bool {
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

fn file_may_contain_value(file: &QueryFile, field: &str, value: &str) -> bool {
    let Some(minimum) = file.min_values.get(field).and_then(|value| value.as_str()) else {
        return true;
    };
    let Some(maximum) = file.max_values.get(field).and_then(|value| value.as_str()) else {
        return true;
    };
    value >= minimum && value <= maximum
}
