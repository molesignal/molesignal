// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Reading a source's window for a scheduled run.

use std::sync::Arc;

use serde_json::Value;

use super::{BootstrapPipelineExecutor, rows_to_objects};
use crate::{
    app::audit_read::{AuditReadCheck, grants_to_read},
    domain::{
        query::{QueryLanguage, QueryRequest, StreamHint},
        stream::StreamType,
    },
    infra::{
        pipeline::{ScheduledPipeline, scheduled::state::Window},
        query::grants::scope,
    },
    shared::{
        Error, Result,
        time::{TimeRange, TimestampMicros},
    },
};

impl BootstrapPipelineExecutor {
    /// Lets pipelines read the audit trail for as long as `check` finds it is allowed.
    pub fn with_audit_read(mut self, check: Arc<dyn AuditReadCheck>) -> Self {
        self.audit_read = Some(check);
        self
    }

    /// One source's window, with what the pipeline may read of it at this moment.
    pub(super) async fn read_window(
        &self,
        pipeline: &ScheduledPipeline,
        window: &Window,
        stream_type: StreamType,
    ) -> Result<Vec<Value>> {
        let grants = grants_to_read(
            self.audit_read.as_deref(),
            &pipeline.org_id,
            pipeline.audit_reader_id.as_ref(),
            &window.source,
        )
        .await?;

        scope(grants, self.read(pipeline, window, stream_type)).await
    }

    /// One source's window. A stream that has never been written has nothing to read.
    async fn read(
        &self,
        pipeline: &ScheduledPipeline,
        window: &Window,
        stream_type: StreamType,
    ) -> Result<Vec<Value>> {
        let req = QueryRequest {
            org_id: pipeline.org_id.clone(),
            language: QueryLanguage::Sql,
            statement: format!("SELECT * FROM {}", window.source),
            time_range: TimeRange::new(TimestampMicros(window.start), TimestampMicros(window.end)),
            stream: Some(StreamHint {
                name: window.source.clone(),
                stream_type,
            }),
            limit: None,
            step: None,
            federation_clusters: Vec::new(),
        };

        match self.query.run_raw(req).await {
            Ok(result) => Ok(rows_to_objects(&result)),
            Err(Error::NotFound(_)) => Ok(Vec::new()),
            Err(e) => Err(e),
        }
    }
}
