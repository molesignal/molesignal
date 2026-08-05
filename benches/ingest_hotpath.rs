// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Microbenchmarks for the in-process write + query hot paths (no server, no IO):
//! the columnar RawEvent->Arrow build, domain->arrow schema mapping + batch
//! alignment, and the PromQL leaf math (rate / histogram_quantile).

use std::{hint::black_box, time::Duration};

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use molesignal::{
    domain::{
        ingestion::RawEvent,
        stream::{FieldDef, FieldType, Schema, StreamDefinition, StreamType},
    },
    infra::{
        ingester::RecordBuilder,
        query::promql::{
            InstantVector, LabelSet, Series, apply_histogram_quantile, apply_rate_like,
        },
        storage::arrow_schema::{align_batch_to_schema, to_arrow},
    },
    shared::{ids::Id, time::TimestampMicros},
};
use serde_json::json;

fn fd(name: &str, ty: FieldType) -> FieldDef {
    FieldDef {
        name: name.into(),
        data_type: ty,
        nullable: true,
        indexed: false,
        encrypted: false,
        exact: false,
    }
}

fn bench_schema() -> Schema {
    Schema {
        fields: vec![
            fd("level", FieldType::Utf8),
            fd("message", FieldType::Utf8),
            fd("service", FieldType::Utf8),
            fd("latency_ms", FieldType::Float64),
            fd("status", FieldType::Int64),
        ],
    }
}

fn stream_def() -> StreamDefinition {
    StreamDefinition {
        id: Id::new(),
        org_id: Id::new(),
        name: "bench".into(),
        stream_type: StreamType::Logs,
        schema: bench_schema(),
        retention: None,
        created_at: TimestampMicros(0),
        updated_at: TimestampMicros(0),
    }
}

fn event(i: u64) -> RawEvent {
    RawEvent {
        timestamp: TimestampMicros(i as i64 * 1_000),
        fields: json!({
            "level": "info",
            "message": format!("req {i}"),
            "service": "checkout",
            "latency_ms": 12.5,
            "status": 200,
        })
        .as_object()
        .unwrap()
        .clone(),
    }
}

fn rate_series(n: usize) -> Vec<Series> {
    let mut labels = LabelSet::new();
    labels.insert("job".into(), "api".into());
    labels.insert("instance".into(), "node-1".into());
    let samples = (0..n).map(|k| (k as i64 * 1_000_000, k as f64)).collect();
    vec![Series { labels, samples }]
}

fn hist_vector() -> InstantVector {
    let les = ["0.1", "0.5", "1.0", "2.5", "5.0", "+Inf"];
    let items = les
        .iter()
        .enumerate()
        .map(|(k, le)| {
            let mut l = LabelSet::new();
            l.insert("le".into(), (*le).to_string());
            (l, (10 * (k + 1)) as f64)
        })
        .collect();
    InstantVector { items }
}

fn bench(c: &mut Criterion) {
    let stream = stream_def();
    let events: Vec<RawEvent> = (0..1_000).map(event).collect();

    // write: RawEvent -> Arrow columnar build.
    c.bench_function("record_builder/push_finish_1000", |b| {
        b.iter_batched(
            || RecordBuilder::new(&stream),
            |mut rb| {
                for (seq, e) in events.iter().enumerate() {
                    rb.push(e, seq as u64).unwrap();
                }
                black_box(rb.finish_and_clear().unwrap())
            },
            BatchSize::SmallInput,
        )
    });

    // schema mapping + batch alignment (schema-evolution path).
    let target = to_arrow(&stream.schema);
    let batch = {
        let mut rb = RecordBuilder::new(&stream);
        for (seq, e) in events.iter().enumerate() {
            rb.push(e, seq as u64).unwrap();
        }
        rb.finish_and_clear().unwrap().0
    };
    c.bench_function("arrow_schema/to_arrow", |b| {
        b.iter(|| black_box(to_arrow(black_box(&stream.schema))))
    });
    c.bench_function("arrow_schema/align_batch_1000", |b| {
        b.iter(|| black_box(align_batch_to_schema(black_box(&batch), black_box(&target)).unwrap()))
    });

    // query: PromQL leaf math over already-built series/vectors.
    let series = rate_series(1_000);
    c.bench_function("promql/rate_1000pts", |b| {
        b.iter_batched(
            || series.clone(),
            |s| black_box(apply_rate_like("rate", s, Duration::from_secs(300))),
            BatchSize::SmallInput,
        )
    });
    let hist = hist_vector();
    c.bench_function("promql/histogram_quantile", |b| {
        b.iter_batched(
            || hist.clone(),
            |h| black_box(apply_histogram_quantile(0.95, h)),
            BatchSize::SmallInput,
        )
    });
}

criterion_group!(benches, bench);
criterion_main!(benches);
