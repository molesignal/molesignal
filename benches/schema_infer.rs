// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Microbenchmarks for the ingest schema-on-write hot paths (no server, no IO):
//! per-batch schema inference and per-event type validation.

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use molesignal::{
    app::ingestion::{check_event_types, infer_schema_extension},
    domain::{
        ingestion::RawEvent,
        stream::{FieldDef, FieldType, Schema, StreamType},
    },
    shared::time::TimestampMicros,
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

fn full_schema() -> Schema {
    Schema {
        fields: vec![
            fd("level", FieldType::Utf8),
            fd("message", FieldType::Utf8),
            fd("service", FieldType::Utf8),
            fd("latency_ms", FieldType::Float64),
            fd("status", FieldType::Int64),
            fd("ok", FieldType::Bool),
        ],
    }
}

fn event(i: u64) -> RawEvent {
    RawEvent {
        timestamp: TimestampMicros(i as i64 * 1_000),
        fields: json!({
            "level": "info",
            "message": format!("event {i}"),
            "service": "checkout",
            "latency_ms": 12.5,
            "status": 200,
            "ok": true,
        })
        .as_object()
        .unwrap()
        .clone(),
    }
}

fn bench(c: &mut Criterion) {
    let events: Vec<RawEvent> = (0..256).map(event).collect();
    let empty = Schema { fields: vec![] };
    let full = full_schema();
    let one = event(1);

    // Worst case: every field is new -> guess + BTreeMap insert per field.
    c.bench_function("infer_schema_extension/all_new_256", |b| {
        b.iter(|| {
            black_box(infer_schema_extension(
                black_box(&empty),
                black_box(&events),
                black_box(StreamType::Logs),
            ))
        })
    });
    // Steady state: all fields known -> early None.
    c.bench_function("infer_schema_extension/all_known_256", |b| {
        b.iter(|| {
            black_box(infer_schema_extension(
                black_box(&full),
                black_box(&events),
                black_box(StreamType::Logs),
            ))
        })
    });
    c.bench_function("check_event_types/match", |b| {
        b.iter(|| black_box(check_event_types(black_box(&full), black_box(&one))))
    });
}

criterion_group!(benches, bench);
criterion_main!(benches);
