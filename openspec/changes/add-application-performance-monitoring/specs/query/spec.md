## ADDED Requirements

### Requirement: Prometheus-Native Exemplar Query

The system SHALL expose authenticated GET and form-encoded POST
`/api/v1/prometheus/api/v1/query_exemplars` endpoints compatible with the Prometheus HTTP API.
The endpoint SHALL accept a PromQL expression plus epoch-seconds or RFC3339 `start` and `end`
values, collect every explicit vector or matrix selector, apply its label matchers, deduplicate
identical remote_write retries and return series labels with bounded Exemplar labels, values and
epoch-second timestamps. Results SHALL remain organization-scoped and query-admission controlled.

#### Scenario: selector returns correlated Trace evidence

- **WHEN** a caller queries Exemplars for
  `http_request_duration_seconds_bucket{service="checkout"}` over a matching time range
- **THEN** only Exemplar rows from matching series are returned
- **AND** `trace_id` and `span_id` labels are preserved exactly

#### Scenario: Exemplar response reaches its safety bound

- **WHEN** a query matches more than 10,000 unique Exemplars
- **THEN** the endpoint returns at most 10,000 unique Exemplars
- **AND** its successful Prometheus response contains a truncation warning
