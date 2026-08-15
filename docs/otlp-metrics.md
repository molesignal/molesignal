# OTLP metric storage contract

MoleSignal flattens every OTLP metric data point into one metric event. Resource,
instrumentation scope, and data-point attributes remain queryable fields. Reserved
metric metadata fields take precedence over attributes with the same name.

| Field | Meaning |
|---|---|
| `metric_name` | OTLP `Metric.name` |
| `metric_kind` | `gauge`, `counter`, `up_down_counter`, `histogram`, `exponential_histogram`, or `summary` |
| `metric_description` | OTLP `Metric.description`, when non-empty |
| `metric_unit` | OTLP `Metric.unit`, when non-empty |
| `metric_temporality` | `delta`, `cumulative`, `unspecified`, or `unknown:<value>` |
| `metric_monotonic` | OTLP `Sum.is_monotonic` |
| `metric_start_time_unix_nano` | Aggregation interval start, when non-zero |
| `value` | Gauge or Sum scalar value, preserving the OTLP numeric representation |

A monotonic OTLP Sum is stored as `counter`; a non-monotonic Sum is stored as
`up_down_counter`. Gauge has no aggregation temporality or monotonicity, and OTLP
defines its start time as ignored, so those fields are absent for Gauge points.
Summary count and sum are cumulative by definition.

MoleSignal preserves DELTA points as received; intake does not convert them into a
cumulative series. PromQL evaluates the rate family according to the stored temporality:

- `increase` sums DELTA values in the query window.
- `rate` divides that sum by the requested range duration.
- `irate` divides the latest DELTA value by its OTLP start/end interval. If an older
  point has no start time, its previous point end is used as a compatibility fallback.

For cumulative monotonic sums, counter-reset handling remains unchanged. Cumulative
non-monotonic sums use their signed first-to-last difference. If an exporter changes
temporality or monotonicity inside one query window, only the latest contiguous run with
the same semantics is evaluated; values from incompatible runs are not mixed.

Metric compaction keeps this contract: DELTA values in one rollup bucket are summed,
the earliest start and latest end delimit the merged interval, and the metadata columns
remain internal query metadata rather than Prometheus labels.
