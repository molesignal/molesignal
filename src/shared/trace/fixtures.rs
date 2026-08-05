// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Canonical Trace fixtures shared by normalization, sampling, storage and exporter tests.

use serde_json::json;

use super::normalization::{CanonicalEvent, CanonicalLink, CanonicalSpan, PartialReason};

fn id(byte: u8, width: usize) -> String {
    format!("{byte:02x}").repeat(width / 2)
}

fn base(trace: u8, span: u8, name: &str, duration_ns: u64) -> CanonicalSpan {
    let mut value = CanonicalSpan::new(
        id(trace, 32),
        id(span, 16),
        name.into(),
        1,
        1_000_000_000,
        1_000_000_000 + duration_ns,
    );
    value
        .resource
        .attributes
        .insert("service.namespace".into(), json!("molesignal"));
    value
        .resource
        .attributes
        .insert("service.name".into(), json!("molesignal-router"));
    value
}

pub fn canonical_http_trace() -> Vec<CanonicalSpan> {
    let mut server = base(1, 1, "GET /api/v1/query", 20_000_000);
    server.kind = 2;
    server
        .attributes
        .insert("http.request.method".into(), json!("GET"));
    server
        .attributes
        .insert("http.route".into(), json!("/api/v1/query"));
    server
        .attributes
        .insert("http.response.status_code".into(), json!(200));

    let mut client = base(1, 2, "POST molesignal.Query/Execute", 15_000_000);
    client.parent_span_id = Some(server.span_id.clone());
    client.kind = 3;
    client.attributes.insert("rpc.system".into(), json!("grpc"));
    vec![server, client]
}

pub fn canonical_grpc_trace() -> Vec<CanonicalSpan> {
    let mut span = base(2, 1, "molesignal.Query/Execute", 25_000_000);
    span.kind = 2;
    span.attributes.insert("rpc.system".into(), json!("grpc"));
    span.attributes
        .insert("rpc.service".into(), json!("molesignal.Query"));
    span.attributes
        .insert("rpc.method".into(), json!("Execute"));
    vec![span]
}

pub fn canonical_sql_trace() -> Vec<CanonicalSpan> {
    let mut span = base(3, 1, "SELECT streams", 3_000_000);
    span.kind = 3;
    span.attributes
        .insert("db.system.name".into(), json!("postgresql"));
    span.attributes
        .insert("db.operation.name".into(), json!("SELECT"));
    span.attributes
        .insert("db.collection.name".into(), json!("streams"));
    span.attributes.insert(
        "molesignal.db.query.fingerprint".into(),
        json!("select_stream_by_id"),
    );
    vec![span]
}

pub fn canonical_object_store_trace() -> Vec<CanonicalSpan> {
    let mut span = base(4, 1, "object_store.get", 8_000_000);
    span.kind = 3;
    span.attributes
        .insert("molesignal.object.operation".into(), json!("get"));
    span.attributes
        .insert("molesignal.object.category".into(), json!("parquet"));
    span.attributes
        .insert("molesignal.object.bytes".into(), json!(8192));
    vec![span]
}

pub fn canonical_async_link_trace() -> Vec<CanonicalSpan> {
    let producer = base(5, 1, "pipeline.enqueue", 1_000_000);
    let mut consumer = base(6, 1, "pipeline.execute", 4_000_000);
    consumer.links.push(CanonicalLink {
        trace_id: producer.trace_id.clone(),
        span_id: producer.span_id.clone(),
        trace_state: String::new(),
        flags: 0,
        attributes: Default::default(),
        dropped_attributes_count: 0,
    });
    vec![producer, consumer]
}

pub fn canonical_streaming_trace() -> Vec<CanonicalSpan> {
    let handshake = base(7, 1, "GET /api/v1/stream", 2_000_000);
    let mut segment = base(8, 1, "stream.session", 30_000_000_000);
    segment.links.push(CanonicalLink {
        trace_id: handshake.trace_id.clone(),
        span_id: handshake.span_id.clone(),
        trace_state: String::new(),
        flags: 0,
        attributes: Default::default(),
        dropped_attributes_count: 0,
    });
    segment.events.push(CanonicalEvent {
        time_unix_nano: segment.start_time_unix_nano + 1_000,
        name: "stream.checkpoint".into(),
        attributes: [("molesignal.stream.messages".into(), json!(1_000))]
            .into_iter()
            .collect(),
        dropped_attributes_count: 0,
    });
    vec![handshake, segment]
}

pub fn canonical_error_trace() -> Vec<CanonicalSpan> {
    let mut span = base(9, 1, "POST /api/v1/ingest", 12_000_000);
    span.status_code = "ERROR".into();
    span.status_message = Some("storage unavailable".into());
    span.attributes
        .insert("error.type".into(), json!("unavailable"));
    vec![span]
}

pub fn canonical_slow_trace() -> Vec<CanonicalSpan> {
    vec![base(10, 1, "object_store.get", 700_000_000)]
}

pub fn canonical_duplicate_trace() -> Vec<CanonicalSpan> {
    let span = base(11, 1, "GET /api/v1/healthz", 100_000);
    vec![span.clone(), span]
}

pub fn canonical_high_fanout_trace(span_count: usize) -> Vec<CanonicalSpan> {
    let count = span_count.max(1);
    let root = base(12, 1, "query.execute", 10_000_000_000);
    let root_id = root.span_id.clone();
    let mut spans = vec![root];
    for index in 1..count {
        let mut child = base(
            12,
            ((index % 250) + 2) as u8,
            "query.shard",
            1_000_000 + index as u64,
        );
        child.span_id = format!("{:016x}", index + 1);
        child.parent_span_id = Some(root_id.clone());
        spans.push(child);
    }
    if spans.len() > 1_000 {
        for span in &mut spans {
            span.mark_partial(PartialReason::SpanLimit);
        }
    }
    spans
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn fixture_catalog_covers_required_shapes() {
        assert_eq!(canonical_http_trace().len(), 2);
        assert_eq!(canonical_grpc_trace().len(), 1);
        assert_eq!(canonical_sql_trace().len(), 1);
        assert_eq!(canonical_object_store_trace().len(), 1);
        assert_eq!(canonical_async_link_trace()[1].links.len(), 1);
        assert_eq!(canonical_streaming_trace()[1].events.len(), 1);
        assert_eq!(canonical_error_trace()[0].status_code, "ERROR");
        assert!(canonical_slow_trace()[0].duration_ns >= 500_000_000);
        assert_eq!(
            canonical_duplicate_trace()[0],
            canonical_duplicate_trace()[1]
        );

        let fanout = canonical_high_fanout_trace(1_001);
        assert_eq!(fanout.len(), 1_001);
        assert_eq!(
            fanout
                .iter()
                .map(|span| span.span_id.as_str())
                .collect::<HashSet<_>>()
                .len(),
            1_001
        );
        assert!(fanout.iter().all(|span| span.partial));
    }
}
