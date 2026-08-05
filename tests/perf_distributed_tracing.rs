// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Release-gate benchmark for default distributed-Trace capture overhead.
//!
//! Run through `scripts/check_distributed_tracing_overhead.sh` on a dedicated, otherwise idle
//! runner. It is ignored by ordinary `cargo test` because scheduler noise in a shared debug
//! runner cannot provide a meaningful percentage gate.

#![cfg(unix)]

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use bytes::Bytes;
use molesignal::{
    config::ObjectStoreSettings,
    domain::{
        ingestion::RawEvent,
        stream::{FieldDef, FieldType, Schema, StreamDefinition, StreamType},
    },
    infra::{
        ingester::RecordBuilder,
        query::promql::{LabelSet, Series, apply_rate_like},
        storage::object::production::ProductionObjectStore,
    },
    shared::{
        ids::Id,
        self_telemetry::{
            ResourceIdentity, SelfTelemetryHub, SelfTelemetryInit, SelfTelemetryLayer,
        },
        time::TimestampMicros,
    },
};
use object_store::{ObjectStoreExt, PutPayload, memory::InMemory, path::Path};
use serde_json::json;
use tracing::{Dispatch, Instrument, instrument::WithSubscriber};
use tracing_subscriber::{layer::SubscriberExt, registry};

struct Workload {
    stream: StreamDefinition,
    events: Vec<RawEvent>,
    query_series: Vec<Series>,
    object_store: Arc<ProductionObjectStore>,
    object_payload: Bytes,
}

impl Workload {
    fn new() -> Self {
        let schema = Schema {
            fields: vec![
                field("level", FieldType::Utf8),
                field("message", FieldType::Utf8),
                field("latency_ms", FieldType::Float64),
                field("status", FieldType::Int64),
            ],
        };
        let stream = StreamDefinition {
            id: Id::new(),
            org_id: Id::new(),
            name: "trace_perf".into(),
            stream_type: StreamType::Logs,
            schema,
            retention: None,
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(0),
        };
        // The tracing contract explicitly requires bounded instrumentation for a
        // 100,000-event ingest batch. Keep that production-sized batch here so
        // fixed per-request Span costs are measured against representative work.
        let events = (0..100_000)
            .map(|index| RawEvent {
                timestamp: TimestampMicros(index * 1_000),
                fields: json!({
                    "level": "info",
                    "message": "representative ingest payload",
                    "latency_ms": 12.5,
                    "status": 200,
                })
                .as_object()
                .expect("event object")
                .clone(),
            })
            .collect();
        let mut labels = LabelSet::new();
        labels.insert("job".into(), "trace-perf".into());
        let query_series = vec![Series {
            labels,
            samples: (0..100_000)
                .map(|index| (index * 1_000_000, index as f64))
                .collect(),
        }];
        let object_store = ProductionObjectStore::wrap(
            Arc::new(InMemory::new()),
            ObjectStoreSettings {
                backend: "local".into(),
                ..ObjectStoreSettings::default()
            },
        );
        Self {
            stream,
            events,
            query_series,
            object_store,
            object_payload: Bytes::from(vec![0x5a; 8 * 1024 * 1024]),
        }
    }

    async fn execute(&self) {
        let ingest = tracing::info_span!(
            "ingest.batch",
            otel.kind = "internal",
            molesignal.ingest.protocol = "benchmark",
        );
        async {
            let mut builder = RecordBuilder::new(&self.stream);
            for (sequence, event) in self.events.iter().enumerate() {
                builder
                    .push(event, sequence as u64)
                    .expect("benchmark event");
            }
            std::hint::black_box(builder.finish_and_clear().expect("benchmark record batch"));
        }
        .instrument(ingest)
        .await;

        let query = tracing::info_span!(
            "query.execute",
            otel.kind = "internal",
            molesignal.query.language = "promql",
        );
        async {
            std::hint::black_box(apply_rate_like(
                "rate",
                self.query_series.clone(),
                Duration::from_secs(300),
            ));
        }
        .instrument(query)
        .await;

        self.object_store
            .put(
                &Path::from("trace-perf/representative.parquet"),
                PutPayload::from(self.object_payload.clone()),
            )
            .await
            .expect("benchmark object put");
        std::hint::black_box(
            self.object_store
                .get(&Path::from("trace-perf/representative.parquet"))
                .await
                .expect("benchmark object get")
                .bytes()
                .await
                .expect("benchmark object bytes"),
        );
    }
}

fn field(name: &str, data_type: FieldType) -> FieldDef {
    FieldDef {
        name: name.into(),
        data_type,
        nullable: true,
        indexed: false,
        encrypted: false,
        exact: false,
    }
}

fn process_cpu_time() -> Duration {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
    // SAFETY: `getrusage` initializes the complete `rusage` structure on success. The pointer is
    // valid for writes and the return code is asserted before `assume_init`.
    let result = unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) };
    assert_eq!(result, 0, "getrusage(RUSAGE_SELF)");
    // SAFETY: guarded by the successful `getrusage` result above.
    let usage = unsafe { usage.assume_init() };
    timeval_duration(usage.ru_utime).saturating_add(timeval_duration(usage.ru_stime))
}

fn timeval_duration(value: libc::timeval) -> Duration {
    Duration::from_secs(value.tv_sec.max(0) as u64)
        .saturating_add(Duration::from_micros(value.tv_usec.max(0) as u64))
}

async fn measure(
    workload: &Workload,
    dispatch: &Dispatch,
    samples: usize,
) -> (Duration, Vec<Duration>) {
    let cpu_started = process_cpu_time();
    let mut latencies = Vec::with_capacity(samples);
    for _ in 0..samples {
        let started = Instant::now();
        workload.execute().with_subscriber(dispatch.clone()).await;
        latencies.push(started.elapsed());
    }
    (process_cpu_time().saturating_sub(cpu_started), latencies)
}

fn p95(mut values: Vec<Duration>) -> Duration {
    values.sort_unstable();
    let index = ((values.len() as f64 * 0.95).ceil() as usize)
        .saturating_sub(1)
        .min(values.len().saturating_sub(1));
    values[index]
}

fn overhead_percent(baseline: Duration, instrumented: Duration) -> f64 {
    if baseline.is_zero() {
        return f64::INFINITY;
    }
    (instrumented.as_secs_f64() / baseline.as_secs_f64() - 1.0) * 100.0
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "release-only performance gate; run scripts/check_distributed_tracing_overhead.sh"]
async fn default_trace_capture_stays_within_cpu_and_p95_budgets() {
    assert_eq!(
        std::env::var("MS_RUN_TRACE_PERF").ok().as_deref(),
        Some("1"),
        "run through scripts/check_distributed_tracing_overhead.sh"
    );
    let samples: usize = std::env::var("MS_TRACE_PERF_SAMPLES")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(40)
        .max(20);
    let workload = Workload::new();
    let baseline = Dispatch::new(tracing::subscriber::NoSubscriber::default());
    let hub = SelfTelemetryHub::new(SelfTelemetryInit {
        queue_capacity: samples.saturating_mul(32),
        logs_enabled: false,
        traces_enabled: true,
        resource: ResourceIdentity::new(
            "molesignal",
            env!("CARGO_PKG_VERSION"),
            "test",
            "standalone",
            "trace-perf",
        ),
    });
    let instrumented = Dispatch::new(registry().with(SelfTelemetryLayer::traces(hub)));

    for _ in 0..3 {
        workload.execute().with_subscriber(baseline.clone()).await;
        workload
            .execute()
            .with_subscriber(instrumented.clone())
            .await;
    }

    let half = samples / 2;
    let (base_cpu_a, mut base_latencies) = measure(&workload, &baseline, half).await;
    let (trace_cpu_a, mut trace_latencies) = measure(&workload, &instrumented, half).await;
    let (trace_cpu_b, trace_latencies_b) = measure(&workload, &instrumented, half).await;
    let (base_cpu_b, base_latencies_b) = measure(&workload, &baseline, half).await;
    base_latencies.extend(base_latencies_b);
    trace_latencies.extend(trace_latencies_b);

    let baseline_cpu = base_cpu_a.saturating_add(base_cpu_b);
    let traced_cpu = trace_cpu_a.saturating_add(trace_cpu_b);
    let baseline_p95 = p95(base_latencies);
    let traced_p95 = p95(trace_latencies);
    let cpu_overhead = overhead_percent(baseline_cpu, traced_cpu);
    let p95_overhead = overhead_percent(baseline_p95, traced_p95);
    eprintln!(
        "distributed Trace overhead: CPU {cpu_overhead:.2}% \
         ({baseline_cpu:?} -> {traced_cpu:?}), P95 {p95_overhead:.2}% \
         ({baseline_p95:?} -> {traced_p95:?}), samples={samples}"
    );

    assert!(
        cpu_overhead <= 5.0,
        "default Trace CPU overhead {cpu_overhead:.2}% exceeds 5%"
    );
    assert!(
        p95_overhead <= 3.0,
        "default Trace P95 latency overhead {p95_overhead:.2}% exceeds 3%"
    );
}
