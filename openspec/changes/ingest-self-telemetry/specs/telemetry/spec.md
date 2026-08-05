## ADDED Requirements

### Requirement: Non-Blocking Internal Telemetry Export Hooks

The telemetry subsystem SHALL support late-bound internal log and span export hooks in addition to its formatter and external OTLP layer. Hooks SHALL be installed before application bootstrap so bounded startup telemetry can be retained, but SHALL not perform database, network, or ingestion work on a tracing callback. Activating or deactivating the hooks SHALL not require replacing the global tracing subscriber.

#### Scenario: Bootstrap event waits in a bounded buffer

- **WHEN** a startup event is emitted after subscriber initialization but before the ingestion service is activated
- **THEN** the event may be retained in the configured bounded startup buffer
- **AND** tracing callback execution performs no database or network I/O

#### Scenario: Self collection can be disabled without rebuilding the subscriber

- **WHEN** `telemetry.self_collect.enabled = false`
- **THEN** the formatter and optional external OTLP exporter continue operating
- **AND** log, metrics, profile, and Trace self-storage workers remain inactive
- **AND** the Trace capture hook MAY remain active to feed the independently configured external exporter

### Requirement: Structured Prometheus Registry Snapshot

The shared metrics registry SHALL expose a structured snapshot API over Prometheus metric families so internal collection can preserve metric type, labels, histogram buckets, summary quantiles, count, and sum without parsing the text returned by `/metrics`. The existing text exposition API SHALL remain unchanged.

#### Scenario: Structured and text snapshots coexist

- **WHEN** a metric family is registered and observed
- **THEN** the structured snapshot returns its typed samples and labels
- **AND** `/metrics` still exposes the same family in Prometheus text format

### Requirement: Self-Exporter Health Metrics

The telemetry subsystem SHALL register bounded-cardinality health metrics for internal export, including queue depth/capacity, accepted and dropped records, batch attempts and failures, last successful write timestamp, and profile availability. Labels SHALL be limited to enumerated signal and reason values and SHALL NOT include organization, stream, metric name, trace ID, span ID, or node ID.

#### Scenario: Exporter metrics remain bounded

- **WHEN** self telemetry contains arbitrary tenants, metric names, and trace IDs
- **THEN** exporter health metric series cardinality is bounded by the documented signal and reason enums
- **AND** no user-controlled identifier appears as a label
