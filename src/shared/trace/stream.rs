// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Bounded tracing for long-lived result streams.
//!
//! The request/RPC span is the handshake. Session work is split into independent root spans,
//! each linked to that handshake, so a slow or abandoned stream cannot keep the request trace
//! open indefinitely. Message values are never inspected or recorded.

use std::{
    pin::Pin,
    task::{Context, Poll},
    time::{Duration, Instant},
};

use futures::Stream;
use tracing::{Span, field};

use crate::shared::trace_context::{SerializedTraceLink, current_trace_context};

pub const STREAM_SEGMENT_MAX_DURATION: Duration = Duration::from_secs(30);
pub const STREAM_SEGMENT_MAX_MESSAGES: u64 = 1_000;

pub struct SegmentedResultStream<S> {
    inner: Pin<Box<S>>,
    handshake: SerializedTraceLink,
    operation: &'static str,
    kind: &'static str,
    max_duration: Duration,
    max_messages: u64,
    segment_index: u64,
    active: Option<ActiveSegment>,
    completed: bool,
}

struct ActiveSegment {
    span: Span,
    started: Instant,
    messages: u64,
}

impl<S> SegmentedResultStream<S> {
    pub fn new(
        inner: S,
        handshake: SerializedTraceLink,
        operation: &'static str,
        kind: &'static str,
    ) -> Self {
        Self::with_limits(
            inner,
            handshake,
            operation,
            kind,
            STREAM_SEGMENT_MAX_DURATION,
            STREAM_SEGMENT_MAX_MESSAGES,
        )
    }

    fn with_limits(
        inner: S,
        handshake: SerializedTraceLink,
        operation: &'static str,
        kind: &'static str,
        max_duration: Duration,
        max_messages: u64,
    ) -> Self {
        Self {
            inner: Box::pin(inner),
            handshake,
            operation,
            kind,
            max_duration,
            max_messages: max_messages.max(1),
            segment_index: 0,
            active: None,
            completed: false,
        }
    }

    fn begin_segment(&mut self) {
        let context = self.handshake.new_execution_root();
        let links = serde_json::json!([{
            "trace_id": self.handshake.trace_id,
            "span_id": self.handshake.span_id,
            "trace_state": self.handshake.trace_state,
            "flags": 0,
            "attributes": {},
            "dropped_attributes_count": 0
        }])
        .to_string();
        let span = tracing::info_span!(
            parent: None,
            "stream.session",
            otel.name = self.operation,
            otel.kind = "consumer",
            otel.trace_id = %context.trace_id,
            otel.span_id = %context.span_id,
            molesignal.stream.kind = self.kind,
            molesignal.stream.segment = self.segment_index,
            molesignal.stream.messages = field::Empty,
            molesignal.stream.duration_ms = field::Empty,
            molesignal.stream.completed = field::Empty,
            molesignal.stream.cancelled = field::Empty,
            error.type = field::Empty,
            links = %links,
        );
        self.active = Some(ActiveSegment {
            span,
            started: Instant::now(),
            messages: 0,
        });
        self.segment_index = self.segment_index.saturating_add(1);
    }

    fn segment_due(&self) -> bool {
        self.active.as_ref().is_some_and(|segment| {
            segment.messages >= self.max_messages || segment.started.elapsed() >= self.max_duration
        })
    }

    fn finish_segment(&mut self, completed: bool, cancelled: bool, failed: bool) {
        let Some(segment) = self.active.take() else {
            return;
        };
        segment
            .span
            .record("molesignal.stream.messages", segment.messages);
        segment.span.record(
            "molesignal.stream.duration_ms",
            segment.started.elapsed().as_millis().min(u64::MAX as u128) as u64,
        );
        segment
            .span
            .record("molesignal.stream.completed", completed);
        segment
            .span
            .record("molesignal.stream.cancelled", cancelled);
        if failed {
            segment.span.record("error.type", "stream_error");
        }
    }
}

impl<S, T, E> Stream for SegmentedResultStream<S>
where
    S: Stream<Item = Result<T, E>>,
{
    type Item = Result<T, E>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        let result = match this.active.as_ref() {
            Some(segment) => {
                let _entered = segment.span.enter();
                this.inner.as_mut().poll_next(cx)
            }
            None => this.inner.as_mut().poll_next(cx),
        };
        match result {
            Poll::Ready(Some(item)) => {
                if this.segment_due() {
                    this.finish_segment(false, false, false);
                }
                if this.active.is_none() {
                    this.begin_segment();
                }
                if let Some(segment) = this.active.as_mut() {
                    segment.messages = segment.messages.saturating_add(1);
                }
                if item.is_err() {
                    this.finish_segment(true, false, true);
                    this.completed = true;
                }
                Poll::Ready(Some(item))
            }
            Poll::Ready(None) => {
                this.finish_segment(true, false, false);
                this.completed = true;
                Poll::Ready(None)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

impl<S> Drop for SegmentedResultStream<S> {
    fn drop(&mut self) {
        if !self.completed {
            self.finish_segment(false, true, false);
        }
    }
}

/// Wrap a stream only when a request/RPC context is active. Background callers without a
/// handshake can use [`segmented_result_stream_with_link`] explicitly.
pub fn segmented_result_stream<S>(
    stream: S,
    operation: &'static str,
    kind: &'static str,
) -> Pin<Box<dyn Stream<Item = S::Item> + Send>>
where
    S: Stream + Send + 'static,
    S::Item: ResultItem + Send + 'static,
    SegmentedResultStream<S>: Stream<Item = S::Item> + Send + 'static,
{
    match current_trace_context() {
        Some(context) => Box::pin(SegmentedResultStream::new(
            stream,
            context.serialized_link(),
            operation,
            kind,
        )),
        None => Box::pin(stream),
    }
}

/// Explicit-link variant for a stream that outlives the task-local handshake.
pub fn segmented_result_stream_with_link<S, T, E>(
    stream: S,
    handshake: SerializedTraceLink,
    operation: &'static str,
    kind: &'static str,
) -> SegmentedResultStream<S>
where
    S: Stream<Item = Result<T, E>>,
{
    SegmentedResultStream::new(stream, handshake, operation, kind)
}

/// Sealed-by-shape marker used to keep the convenience wrapper's item type generic.
pub trait ResultItem {}

impl<T, E> ResultItem for Result<T, E> {}

#[cfg(test)]
mod tests {
    use futures::{StreamExt, stream};
    use tracing_subscriber::prelude::*;

    use super::*;
    use crate::shared::{
        self_telemetry::{
            ResourceIdentity, SelfTelemetryHub, SelfTelemetryInit, SelfTelemetryLayer,
            SelfTelemetrySignal,
        },
        trace_context::TraceContext,
    };

    fn telemetry() -> (
        Arc<SelfTelemetryHub>,
        tokio::sync::mpsc::Receiver<crate::domain::ingestion::RawEvent>,
    ) {
        let hub = SelfTelemetryHub::new(SelfTelemetryInit {
            queue_capacity: 16,
            traces_enabled: true,
            resource: ResourceIdentity::new("molesignal", "test", "test", "router", "node"),
        });
        let traces = hub.take_receiver(SelfTelemetrySignal::Traces).unwrap();
        (hub, traces)
    }

    use std::sync::Arc;

    #[test]
    fn rolls_over_at_message_limit_and_links_each_new_root() {
        let (hub, mut traces) = telemetry();
        let subscriber = tracing_subscriber::registry().with(SelfTelemetryLayer::traces(hub));
        let handshake = TraceContext::new_root("request-1").serialized_link();

        tracing::subscriber::with_default(subscriber, || {
            futures::executor::block_on(async {
                let stream = stream::iter((0..5).map(Ok::<_, ()>));
                let segmented = SegmentedResultStream::with_limits(
                    stream,
                    handshake.clone(),
                    "test.stream",
                    "test",
                    Duration::from_secs(30),
                    2,
                );
                assert_eq!(segmented.collect::<Vec<_>>().await.len(), 5);
            })
        });

        let mut rows = Vec::new();
        while let Ok(row) = traces.try_recv() {
            rows.push(row);
        }
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].fields["molesignal.stream.messages"], 2);
        assert_eq!(rows[1].fields["molesignal.stream.messages"], 2);
        assert_eq!(rows[2].fields["molesignal.stream.messages"], 1);
        for row in &rows {
            let links = row.fields["links"].as_array().unwrap();
            assert_eq!(links[0]["trace_id"], handshake.trace_id);
            assert_eq!(links[0]["span_id"], handshake.span_id);
        }
        assert_ne!(rows[0].fields["trace_id"], rows[1].fields["trace_id"]);
    }

    #[test]
    fn drop_marks_an_open_segment_cancelled() {
        let (hub, mut traces) = telemetry();
        let subscriber = tracing_subscriber::registry().with(SelfTelemetryLayer::traces(hub));
        let handshake = TraceContext::new_root("request-1").serialized_link();

        tracing::subscriber::with_default(subscriber, || {
            futures::executor::block_on(async {
                let mut segmented = SegmentedResultStream::new(
                    stream::iter([Ok::<_, ()>(1), Ok(2)]),
                    handshake,
                    "test.stream",
                    "test",
                );
                assert_eq!(segmented.next().await, Some(Ok(1)));
            })
        });

        let row = traces.try_recv().unwrap();
        assert_eq!(row.fields["molesignal.stream.cancelled"], true);
        assert_eq!(row.fields["molesignal.stream.completed"], false);
    }
}
