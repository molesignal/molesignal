## ADDED Requirements

### Requirement: Connector Trait and Registry

The system SHALL define a `Connector` trait with `kind() -> &str`, `validate_config(&self, config: &Value) -> Result<()>`, and either `pull(&self, ctx: &PullCtx) -> Result<()>` (pull-mode) or `accept_push(&self, ctx: &PushCtx, payload: Bytes) -> Result<()>` (push-mode). Connectors are persisted in `connectors { id, org_id, name, kind, config_json, enabled, last_run_at?, last_error?, created_at, updated_at }`. `GET/POST/PUT/DELETE /api/v1/connectors` (Admin+) manage rows; on enable, the scheduler picks up the row within `connector.poll_interval_secs` (default 30s).

#### Scenario: Connector CRUD
- **WHEN** an Admin POSTs `{ name: "prod-cloudwatch", kind: "aws_cloudwatch_logs", config: { region, log_group, role_arn?, access_key, secret_key } }`
- **THEN** the response is `201 Created`; the next scheduler tick begins pulling

### Requirement: AWS CloudWatch Logs Pull Connector

The `aws_cloudwatch_logs` connector SHALL poll `FilterLogEvents` every `config.poll_interval_secs` (default 30) using static `access_key`/`secret_key`, paginate via `nextToken`, and stream events through `IngestService::ingest` into a target stream resolved from `config.target_stream` (default `cloudwatch_<log_group>`). The connector SHALL persist `last_event_ts` in `connectors.config_json` to avoid re-reading.

#### Scenario: Poll picks up new events
- **WHEN** the scheduler tick fires and CloudWatch has 100 new events since `last_event_ts`
- **THEN** all 100 events are ingested; `last_event_ts` advances to the most recent event's timestamp

### Requirement: Cloudflare Logpush and Heroku Drain HTTP Receivers

The system SHALL expose `POST /api/v1/_cloudflare` (Cloudflare Logpush NDJSON) and `POST /api/v1/_heroku` (Heroku Logplex syslog-octet-framed) push endpoints; both authenticate via an `X-Connector-Token` header that matches the corresponding `connectors.config.push_token`, and ingest events into `config.target_stream`.

#### Scenario: Cloudflare push accepted
- **WHEN** Cloudflare delivers a 1000-line NDJSON batch with valid `X-Connector-Token`
- **THEN** 1000 events are ingested into the configured stream; response `200 OK`

#### Scenario: Bad token rejected
- **WHEN** `X-Connector-Token` does not match
- **THEN** response is `401 Unauthorized`, no events ingested, audit row written

### Requirement: Kinesis Firehose Connector Binding

The system SHALL allow a Firehose request to opt into connector-mediated routing by passing `?connector_id=<id>` on `POST /api/v1/_kinesis_firehose`; when present, the request MUST be authenticated against the matching `connectors` row's `push_token` (rejecting with `401 Unauthorized` on mismatch) and the events SHALL be routed to that connector's `config.target_stream` with `last_run_at` updated atomically.

#### Scenario: Firehose with connector binding
- **WHEN** a Firehose request includes `?connector_id=<id>`, presents a matching `X-Connector-Token`, and the connector is enabled
- **THEN** the events route to that connector's `config.target_stream`, `last_run_at = now`, and the response is the standard Firehose ack

#### Scenario: Firehose without connector_id falls back to query stream
- **WHEN** a Firehose request omits `?connector_id` but supplies `?stream=<name>`
- **THEN** the events route to that stream per the base `ingest-protocols` requirement, no `connectors` row is touched
