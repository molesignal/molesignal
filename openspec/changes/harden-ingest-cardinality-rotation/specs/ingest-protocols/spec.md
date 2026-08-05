## MODIFIED Requirements

### Requirement: Prometheus remote_write Receiver

The system SHALL accept `POST /api/v1/prometheus/api/v1/write` carrying `Content-Encoding: snappy` and `Content-Type: application/x-protobuf` payloads matching `prometheus.WriteRequest`. Before schema evolution, WAL append, or per-sample label cloning, it SHALL validate every `TimeSeries` against configurable limits for non-`__name__` label count, label-name bytes, and label-value bytes; reject duplicate, empty, or storage-reserved label names; and require exactly one non-empty `__name__`. After the complete request passes structural preflight, the system SHALL translate samples to metrics rows, split each metric's rows into internal batches of at most `ingester.prometheus.max_samples_per_batch`, route those batches through `IngestService::ingest`, and acknowledge complete success with `204 No Content`. Raw metric labels MUST NOT be silently dropped or folded into an overflow series.

#### Scenario: remote_write batch ingested in bounded chunks
- **WHEN** a Prometheus server pushes 40,000 valid samples for one metric and `max_samples_per_batch = 16,384`
- **THEN** the adapter submits three ordered internal batches, each receives an independent WAL sequence, and the response is `204 No Content`

#### Scenario: Bad snappy payload rejected
- **WHEN** the payload Content-Encoding declares snappy but bytes fail snappy decoding
- **THEN** the response is `400 Bad Request` with `{ "error": "snappy decode failed: ..." }`

#### Scenario: Excessive label dimensions are rejected before persistence
- **WHEN** any `TimeSeries` contains more non-`__name__` labels than `max_labels_per_series`
- **THEN** the complete request is rejected with `400 Bad Request`
- **AND** no stream schema, WAL segment, Arrow buffer, or parquet file is changed

#### Scenario: Duplicate or reserved label name is rejected
- **WHEN** a `TimeSeries` repeats a label name or uses `value` or `_timestamp` as a label name
- **THEN** the complete request is rejected before persistence with a bounded rejection reason metric

#### Scenario: Oversized label is rejected
- **WHEN** a label name or value exceeds its configured UTF-8 byte limit
- **THEN** the complete request is rejected before persistence and the offending label value is not emitted in logs or metrics
