// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! SQL 引擎内部物理数据集选择与 QueryFile 合并。

use std::{collections::HashMap, sync::Arc};

use arrow::array::RecordBatch;
use futures::future::try_join_all;

use crate::{
    domain::{
        storage::{DatasetTypeId, QueryFile, QueryFileSource},
        stream::{StreamDefinition, StreamType},
    },
    infra::query::catalog_source::{CatalogQuerySource, StreamStorageSnapshot},
    shared::{Result, ids::Id, time::TimeRange},
};

pub(super) struct LoadedDataset {
    pub files: Vec<QueryFile>,
    /// Explicit Primary Artifact key → ready Tantivy Artifact key relationships.
    pub tantivy_indexes: HashMap<String, String>,
    pub buffered_batches: Vec<RecordBatch>,
}

pub(super) struct DatasetLoadSelection<'a> {
    pub organization_id: &'a Id,
    pub stream_name: &'a str,
    pub stream_type: StreamType,
    pub dataset_type: Option<DatasetTypeId>,
    pub time_range: TimeRange,
}

impl LoadedDataset {
    pub(super) fn from_catalog_snapshot(snapshot: StreamStorageSnapshot) -> Self {
        let mut files = snapshot.files();
        files.sort_by(|left, right| {
            right
                .time_range
                .end
                .cmp(&left.time_range.end)
                .then_with(|| right.id.0.cmp(&left.id.0))
        });
        files.dedup_by(|left, right| left.id == right.id);
        Self {
            files,
            tantivy_indexes: snapshot.tantivy_indexes(),
            buffered_batches: snapshot.buffered_batches(),
        }
    }
}

pub(super) async fn load(
    repository: &Arc<dyn QueryFileSource>,
    catalog_source: Option<&Arc<CatalogQuerySource>>,
    explicit_tantivy_indexes: &HashMap<String, String>,
    definition: Option<&StreamDefinition>,
    selection: DatasetLoadSelection<'_>,
) -> Result<LoadedDataset> {
    let DatasetLoadSelection {
        organization_id,
        stream_name,
        stream_type,
        dataset_type,
        time_range,
    } = selection;
    if let (Some(source), Some(definition)) = (catalog_source, definition) {
        let dataset_types = match dataset_type.clone() {
            Some(dataset_type) => vec![dataset_type],
            None => crate::domain::storage::logical_query_dataset_types(stream_type)?,
        };
        let snapshot = source
            .snapshot_stream(definition, &dataset_types, time_range)
            .await?;
        return Ok(LoadedDataset::from_catalog_snapshot(snapshot));
    }
    let (mut files, buffered_batches) = if let Some(dataset_type) = dataset_type {
        (
            repository
                .find_dataset(
                    organization_id,
                    stream_name,
                    stream_type,
                    dataset_type,
                    time_range,
                )
                .await?,
            Vec::new(),
        )
    } else {
        let lookups = crate::domain::storage::logical_query_dataset_types(stream_type)?
            .into_iter()
            .map(|dataset_type| {
                repository.find_dataset(
                    organization_id,
                    stream_name,
                    stream_type,
                    dataset_type,
                    time_range,
                )
            });
        (
            try_join_all(lookups).await?.into_iter().flatten().collect(),
            Vec::new(),
        )
    };
    files.sort_by(|left, right| {
        right
            .time_range
            .end
            .cmp(&left.time_range.end)
            .then_with(|| right.id.0.cmp(&left.id.0))
    });
    files.dedup_by(|left, right| left.id == right.id);
    Ok(LoadedDataset {
        files,
        tantivy_indexes: explicit_tantivy_indexes.clone(),
        buffered_batches,
    })
}
