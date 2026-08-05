## MODIFIED Requirements

### Requirement: Prometheus remote_write Receiver

The system SHALL accept `POST /api/v1/prometheus/api/v1/write` carrying
`Content-Encoding: snappy` and `Content-Type: application/x-protobuf` payloads matching
`prometheus.WriteRequest`. Before persistence it SHALL validate metric-series labels and native
`TimeSeries.exemplars` with bounded label counts/name/value sizes, reject duplicate or reserved
series labels and reject non-finite Exemplar values. Samples SHALL become ordinary metric rows.
Exemplars SHALL retain their series labels, Exemplar labels, value and event timestamp in the
owning metric stream without becoming ordinary PromQL samples.

#### Scenario: remote_write batch ingested

- **WHEN** a Prometheus server pushes 1,000 samples across 3 metrics
- **THEN** the response is `204 No Content`
- **AND** three metrics streams are touched and auto-created on first write

#### Scenario: Bad snappy payload rejected

- **WHEN** the payload Content-Encoding declares snappy but bytes fail snappy decoding
- **THEN** the response is `400 Bad Request`
- **AND** no sample or Exemplar is persisted

#### Scenario: remote_write batch contains a native Exemplar

- **WHEN** one remote_write TimeSeries contains a sample and an Exemplar carrying `trace_id` and
  `span_id`
- **THEN** the sample remains queryable through ordinary PromQL
- **AND** the Exemplar is queryable through Prometheus `query_exemplars`
- **AND** the Exemplar does not create an additional PromQL sample

#### Scenario: malformed Exemplar is rejected before persistence

- **WHEN** an Exemplar contains duplicate label names, exceeds configured label limits, or carries
  a non-finite value
- **THEN** the complete remote_write request is rejected
- **AND** no sample or Exemplar from the request is persisted
