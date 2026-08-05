# Ingest Protocols Capability

## Purpose

OTLP / Prometheus remote_write / Elasticsearch Bulk / Loki / Syslog / Kinesis Firehose 等多协议接收器，统一桥接到 `IngestService::ingest`。

## Requirements

### Requirement: OTLP gRPC and HTTP Receivers

The system SHALL implement OTLP receivers for logs, metrics, and traces over both gRPC and HTTP/protobuf via the upstream `opentelemetry-proto` definitions, mounted at gRPC services `opentelemetry.proto.collector.{logs,metrics,traces}.v1.{Logs,Metrics,Trace}Service` and at HTTP paths `POST /api/v1/{logs,metrics,traces}` (Content-Type `application/x-protobuf` or `application/json`). Every accepted record SHALL be converted to a `RawEvent` and routed through `IngestService::ingest` with `stream_type` derived from the signal kind.

#### Scenario: OTLP gRPC logs accepted
- **WHEN** an OTLP collector sends `ExportLogsServiceRequest` with 50 log records via gRPC
- **THEN** the 50 records are translated and accepted; the response is `ExportLogsServiceResponse { partial_success: empty }`; `ingest_protocol_records_total{protocol="otlp_grpc", kind="logs"} += 50`

#### Scenario: OTLP HTTP JSON traces accepted
- **WHEN** a client POSTs OTLP-JSON traces to `/api/v1/traces` with `Content-Type: application/json`
- **THEN** the response is `200 OK`; trace records are written to a stream named after the resource attribute `service.name`, auto-creating the stream if absent

### Requirement: Prometheus remote_write Receiver

The system SHALL accept `POST /api/v1/prometheus/api/v1/write` carrying `Content-Encoding: snappy` and `Content-Type: application/x-protobuf` payloads matching `prometheus.WriteRequest`, translate each `TimeSeries` to `(labels_json, _timestamp, value)` rows on a metrics stream named after `__name__`, and acknowledge with `204 No Content`.

#### Scenario: remote_write batch ingested
- **WHEN** a Prometheus server pushes 1,000 samples across 3 metrics
- **THEN** the response is `204 No Content`; three metrics streams are touched (auto-created on first write); `ingest_protocol_records_total{protocol="prom_rw"} += 1000`

#### Scenario: Bad snappy payload rejected
- **WHEN** the payload Content-Encoding declares snappy but bytes fail snappy decoding
- **THEN** the response is `400 Bad Request` with `{ "error": "snappy decode failed: ..." }`

### Requirement: Elasticsearch Bulk-Compatible Receivers

The system SHALL accept `POST /api/v1/_bulk`, `/api/v1/_json`, and `/api/v1/_multi` using the Elasticsearch bulk and NDJSON conventions (action-line + source-line for `_bulk`, JSON object per line for `_json`, multiple JSON objects per request for `_multi`), and return a response shape compatible with the ES bulk response (`{ took, errors, items: [{ index: { _index, status } }, ...] }`) so existing ES clients (Vector, Fluent Bit ES output) work unchanged.

#### Scenario: Elasticsearch bulk write succeeds
- **WHEN** Vector posts a 5-record `_bulk` with `index` actions targeting stream `app`
- **THEN** the response is `200 OK` with `{ errors: false, items: [ { index: { _index: "app", status: 200 } } * 5 ] }`

#### Scenario: Per-document failure surfaced
- **WHEN** one record in a 10-record `_bulk` fails schema validation
- **THEN** the response has `errors: true` and the corresponding `items[i].index.status = 400` with `error.reason`

### Requirement: Loki Push Receiver

The system SHALL accept `POST /api/v1/loki/api/v1/push` payloads (snappy-compressed protobuf or JSON per Loki spec) and route each `Stream` to a logs stream whose name is derived from the `service_name` / `job` label (falling back to `loki_default`), preserving all other labels as event fields.

#### Scenario: Loki push from Promtail
- **WHEN** Promtail sends a snappy-protobuf push with 100 lines across 2 label sets
- **THEN** all 100 records are accepted; `ingest_protocol_records_total{protocol="loki"} += 100`; labels are stored as event fields

### Requirement: Syslog UDP/TCP Listeners

When `[syslog].udp_bind` or `tcp_bind` is set, the system SHALL bind that address and parse incoming RFC3164/RFC5424 messages via `syslog-loose`, mapping `(facility, severity, hostname, app, msg, structured_data, timestamp)` to events on stream `[syslog].default_stream` (default `syslog`).

#### Scenario: RFC5424 message accepted via UDP
- **WHEN** a client sends a well-formed RFC5424 syslog message to the UDP port
- **THEN** the message is parsed and inserted into the configured stream; `ingest_protocol_records_total{protocol="syslog_udp"} += 1`

#### Scenario: Malformed line dropped with metric
- **WHEN** a line fails both RFC3164 and RFC5424 parsers
- **THEN** the line is dropped, `ingest_protocol_parse_errors_total{protocol="syslog"} += 1`, no event is written

### Requirement: Kinesis Firehose Receiver

The system SHALL accept `POST /api/v1/_kinesis_firehose` per the AWS Firehose HTTP delivery contract, Base64-decode each record's `data` field, split on newline boundaries to extract individual JSON events, and respond with the `{ requestId, timestamp }` shape Firehose expects.

#### Scenario: Firehose delivery accepted
- **WHEN** Firehose pushes 3 records each containing 4 newline-separated JSON events
- **THEN** the response is `200 OK` with `{ requestId, timestamp }`, 12 events are inserted into the target stream resolved from `?stream=<name>`
