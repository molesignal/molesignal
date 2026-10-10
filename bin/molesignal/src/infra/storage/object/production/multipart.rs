// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! multipart 上传的观测包装：part/complete/abort 以有界 Event 记录，不为每个 part 创建子 Span。

use std::fmt;

use futures::FutureExt;
use object_store::{
    MultipartUpload, PutPayload, PutResult, Result as ObjectStoreResult, UploadPart,
};
use tracing::Instrument;

use super::telemetry::{bytes_total, error_reason, errors_total, ops_total};

pub(super) struct InstrumentedMultipartUpload {
    inner: Box<dyn MultipartUpload>,
    span: tracing::Span,
    backend: &'static str,
    parts: u32,
    bytes: u64,
}

impl fmt::Debug for InstrumentedMultipartUpload {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("InstrumentedMultipartUpload")
            .field("backend", &self.backend)
            .field("parts", &self.parts)
            .field("bytes", &self.bytes)
            .finish_non_exhaustive()
    }
}

impl InstrumentedMultipartUpload {
    pub(super) fn new(
        inner: Box<dyn MultipartUpload>,
        span: tracing::Span,
        backend: &'static str,
    ) -> Self {
        Self {
            inner,
            span,
            backend,
            parts: 0,
            bytes: 0,
        }
    }
}

#[async_trait::async_trait]
impl MultipartUpload for InstrumentedMultipartUpload {
    fn put_part(&mut self, data: PutPayload) -> UploadPart {
        self.parts = self.parts.saturating_add(1);

        self.bytes = self
            .bytes
            .saturating_add(data.content_length().try_into().unwrap_or(u64::MAX));

        let part = self.parts;

        if part <= 128 {
            self.span.in_scope(|| {
                tracing::info!(
                    molesignal.span_event = true,
                    otel.event.name = "object_store.multipart_part",
                    multipart.part_number = part,
                    multipart.part_bytes = data.content_length() as u64,
                );
            });
        }

        let future = self.inner.put_part(data);
        let span = self.span.clone();

        async move {
            let result = future.instrument(span.clone()).await;

            if let Err(error) = &result {
                span.record("error.type", error_reason(error));
            }

            result
        }
        .boxed()
    }

    async fn complete(&mut self) -> ObjectStoreResult<PutResult> {
        let span = self.span.clone();
        let result = self.inner.complete().instrument(span.clone()).await;
        span.record("molesignal.object.bytes", self.bytes);
        span.record("molesignal.object.retry_count", 0);

        span.in_scope(|| {
            tracing::info!(
                molesignal.span_event = true,
                otel.event.name = "object_store.multipart_complete",
                multipart.part_count = self.parts,
                multipart.total_bytes = self.bytes,
            );
        });

        match &result {
            Ok(_) => {
                ops_total()
                    .with_label_values(&[self.backend, "multipart_complete"])
                    .inc();

                bytes_total()
                    .with_label_values(&[self.backend, "multipart_complete"])
                    .inc_by(self.bytes);
            }
            Err(error) => {
                let reason = error_reason(error);
                span.record("error.type", reason);

                errors_total()
                    .with_label_values(&[self.backend, "multipart_complete", reason])
                    .inc();
            }
        }

        result
    }

    async fn abort(&mut self) -> ObjectStoreResult<()> {
        let span = self.span.clone();
        let result = self.inner.abort().instrument(span.clone()).await;

        span.in_scope(|| {
            tracing::info!(
                molesignal.span_event = true,
                otel.event.name = "object_store.multipart_abort",
                multipart.part_count = self.parts,
            );
        });

        if let Err(error) = &result {
            span.record("error.type", error_reason(error));
        }

        result
    }
}
