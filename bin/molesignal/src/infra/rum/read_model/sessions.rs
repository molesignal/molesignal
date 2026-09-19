// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Cursor-paginated session Top-K scan over the physical RUM session summary.

use std::{cmp::Ordering, collections::HashSet};

use futures::StreamExt;

use super::{
    model::{RumSessionRecord, SESSION_COLUMNS},
    scan::RumReadModelReader,
};
use crate::{
    domain::storage::{DatasetTypeId, type_id::builtin},
    infra::storage::parquet::reader::{ParquetReader, ReadOptions},
    shared::{Error, Result, cursor::CursorDirection, ids::Id, time::TimeRange},
};

#[derive(Clone, Copy, Debug)]
pub struct SessionPageBoundary<'a> {
    pub direction: CursorDirection,
    pub started_at_micros: i64,
    pub session_id: &'a str,
    pub event_id: &'a str,
}

#[derive(Clone, Copy, Debug)]
pub struct SessionPageQuery<'a> {
    pub text: Option<&'a str>,
    pub country: Option<&'a str>,
    pub browser: Option<&'a str>,
    pub allowed_session_ids: Option<&'a HashSet<String>>,
    pub boundary: Option<SessionPageBoundary<'a>>,
    pub limit: usize,
}

impl RumReadModelReader {
    pub async fn session_page(
        &self,
        org_id: &Id,
        range: TimeRange,
        query: SessionPageQuery<'_>,
    ) -> Result<Vec<RumSessionRecord>> {
        if query.limit == 0 {
            return Ok(Vec::new());
        }
        let mut input = self
            .source_files(
                org_id,
                "rum_sessions",
                DatasetTypeId::builtin(builtin::DATASET_RUM_SESSION_SUMMARY),
                range,
            )
            .await?;
        let before = query
            .boundary
            .is_some_and(|boundary| boundary.direction == CursorDirection::Before);
        input.files.sort_by(|left, right| {
            let ordering = right
                .meta
                .time_range
                .end
                .cmp(&left.meta.time_range.end)
                .then_with(|| right.meta.object_key.cmp(&left.meta.object_key));
            if before { ordering.reverse() } else { ordering }
        });

        let mut top = Vec::with_capacity(query.limit);
        for buffered in input.batches {
            for row in 0..buffered.batch.num_rows() {
                let Some(record) = RumSessionRecord::from_batch(&buffered.batch, row) else {
                    continue;
                };
                if in_range(record.timestamp_micros, buffered.range)
                    && matches_session(&record, query)
                    && matches_boundary(&record, query.boundary)
                {
                    push_top_k(&mut top, record, query.limit, before);
                }
            }
        }
        let reader = ParquetReader::new(self.object_store.clone());
        for (index, file) in input.files.iter().enumerate() {
            let options = ReadOptions::new()
                .with_time_range(file.range.start.0, file.range.end.0)
                .with_columns(SESSION_COLUMNS)
                .with_known_size(file.meta.size_bytes);
            let mut batches = match reader
                .stream_from_store(self.object_store.clone(), &file.meta.object_key, options)
                .await
            {
                Ok(batches) => batches,
                Err(Error::NotFound(_)) => {
                    tracing::warn!(
                        object_key = %file.meta.object_key,
                        "RUM session summary parquet is missing"
                    );
                    continue;
                }
                Err(error) => return Err(error),
            };
            while let Some(batch) = batches.next().await {
                let batch = batch?;
                for row in 0..batch.num_rows() {
                    let Some(record) = RumSessionRecord::from_batch(&batch, row) else {
                        continue;
                    };
                    if !in_range(record.timestamp_micros, file.range)
                        || !matches_session(&record, query)
                        || !matches_boundary(&record, query.boundary)
                    {
                        continue;
                    }
                    push_top_k(&mut top, record, query.limit, before);
                }
            }
            if !before
                && top.len() >= query.limit
                && input.files.get(index + 1).is_some_and(|next| {
                    top.iter()
                        .max_by(|left, right| effective_cmp(left, right, before))
                        .is_some_and(|worst| next.meta.time_range.end.0 < worst.timestamp_micros)
                })
            {
                break;
            }
        }
        top.sort_by(|left, right| effective_cmp(left, right, before));
        Ok(top)
    }
}

fn in_range(timestamp: i64, range: TimeRange) -> bool {
    timestamp >= range.start.0 && timestamp < range.end.0
}

fn matches_session(record: &RumSessionRecord, query: SessionPageQuery<'_>) -> bool {
    if query
        .country
        .is_some_and(|value| record.country.as_deref() != Some(value))
        || query
            .browser
            .is_some_and(|value| record.browser.as_deref() != Some(value))
        || query
            .allowed_session_ids
            .is_some_and(|ids| !ids.contains(&record.session_id))
    {
        return false;
    }
    let Some(text) = query.text else {
        return true;
    };
    let text = text.to_ascii_lowercase();
    [
        Some(record.session_id.as_str()),
        record.user_id.as_deref(),
        record.country.as_deref(),
        record.browser.as_deref(),
        record.application.as_deref(),
        record.environment.as_deref(),
        record.version.as_deref(),
        record.landing_page.as_deref(),
        record.last_page.as_deref(),
    ]
    .into_iter()
    .flatten()
    .any(|value| value.to_ascii_lowercase().contains(&text))
}

fn canonical_cmp(left: &RumSessionRecord, right: &RumSessionRecord) -> Ordering {
    right
        .timestamp_micros
        .cmp(&left.timestamp_micros)
        .then_with(|| right.session_id.cmp(&left.session_id))
        .then_with(|| right.event_id.cmp(&left.event_id))
}

fn effective_cmp(left: &RumSessionRecord, right: &RumSessionRecord, before: bool) -> Ordering {
    let ordering = canonical_cmp(left, right);
    if before { ordering.reverse() } else { ordering }
}

fn matches_boundary(record: &RumSessionRecord, boundary: Option<SessionPageBoundary<'_>>) -> bool {
    let Some(boundary) = boundary else {
        return true;
    };
    let position = (
        boundary.started_at_micros,
        boundary.session_id,
        boundary.event_id,
    );
    let row = (
        record.timestamp_micros,
        record.session_id.as_str(),
        record.event_id.as_str(),
    );
    match boundary.direction {
        CursorDirection::After => row < position,
        CursorDirection::Before => row > position,
    }
}

fn push_top_k(
    top: &mut Vec<RumSessionRecord>,
    candidate: RumSessionRecord,
    limit: usize,
    before: bool,
) {
    if let Some(existing) = top
        .iter()
        .position(|row| row.session_id == candidate.session_id)
    {
        if effective_cmp(&candidate, &top[existing], before) == Ordering::Less {
            top[existing] = candidate;
        }
        return;
    }
    if top.len() < limit {
        top.push(candidate);
        return;
    }
    let worst = top
        .iter()
        .enumerate()
        .max_by(|(_, left), (_, right)| effective_cmp(left, right, before))
        .map(|(index, _)| index)
        .unwrap_or_default();
    if effective_cmp(&candidate, &top[worst], before) == Ordering::Less {
        top[worst] = candidate;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(id: &str, timestamp: i64) -> RumSessionRecord {
        RumSessionRecord {
            timestamp_micros: timestamp,
            event_id: format!("event-{id}"),
            session_id: id.into(),
            user_id: None,
            ip_address: None,
            duration_ms: None,
            application: None,
            service: None,
            environment: None,
            version: None,
            country: None,
            browser: None,
            device: None,
            os: None,
            landing_page: None,
            last_page: None,
            view_count: None,
            action_count: None,
            error_count: 0,
            trace_id: None,
        }
    }

    #[test]
    fn top_k_is_stable_and_deduplicated() {
        let mut rows = Vec::new();
        push_top_k(&mut rows, session("a", 10), 2, false);
        push_top_k(&mut rows, session("b", 30), 2, false);
        push_top_k(&mut rows, session("a", 20), 2, false);
        push_top_k(&mut rows, session("c", 5), 2, false);
        rows.sort_by(canonical_cmp);
        assert_eq!(
            rows.iter()
                .map(|row| row.session_id.as_str())
                .collect::<Vec<_>>(),
            vec!["b", "a"]
        );
        assert_eq!(rows[1].timestamp_micros, 20);
    }
}
