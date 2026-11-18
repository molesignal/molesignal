// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Columnar batches reach the intake role here: written like row batches, then checked for
//! rotation.

use async_trait::async_trait;
use domain::{intake::IntakeResult, storage::DatasetTypeId};
use kernel::Result;

use super::IntakeWorker;
use crate::{
    app::intake::{ColumnarBatch, ColumnarSink},
    infra::intake::wal_record::WalBatch,
};

#[async_trait]
impl ColumnarSink for IntakeWorker {
    async fn write_columns(
        &self,
        dataset_type: DatasetTypeId,
        batch: ColumnarBatch,
    ) -> Result<IntakeResult> {
        let stream = self
            .streams
            .get(&batch.org_id, &batch.stream, batch.stream_type)
            .await?;

        let key = self
            .datasets
            .resolve(&stream, dataset_type.clone())
            .await?
            .dataset
            .id
            .clone();

        let result = self
            .sink
            .write_columns(
                dataset_type,
                WalBatch {
                    org_id: batch.org_id,
                    stream: batch.stream,
                    stream_type: batch.stream_type,
                    collected_bytes: batch.collected_bytes,
                    columns: batch.columns,
                },
            )
            .await?;

        self.notify_when_rotation_due(&key).await;
        Ok(result)
    }
}
