## ADDED Requirements

### Requirement: RUM Receivers

The system SHALL expose `POST /api/v1/rum/{sessions,actions,errors,replay}` accepting Datadog-RUM-compatible JSON payloads (one event per JSON object, NDJSON also supported), routing each event kind to a derived stream `rum_{sessions,actions,errors,replay_events}`. Replay events SHALL additionally be archived as NDJSON+zstd objects at object_store key `rum/<org>/<session_id>/<seq>.replay.ndjson.zst`.

#### Scenario: Session start accepted
- **WHEN** a browser SDK POSTs a session-start event to `/api/v1/rum/sessions` with `{ session_id, user_id?, app_version, started_at }`
- **THEN** the event is inserted into `rum_sessions` and the response is `200 OK`

#### Scenario: Replay events archived as object
- **WHEN** the SDK POSTs a `replay` batch of 200 rrweb-like events for `session_id = ses-123`
- **THEN** an object key `rum/<org>/ses-123/0001.replay.ndjson.zst` is written; a row in `rum_replay_events` references the object key with `seq=1`

### Requirement: RUM Session-to-Trace Correlation

When a RUM action carries `trace_id`, the system SHALL include that trace_id in the `rum_actions` row so queries can join RUM actions to backend traces by `trace_id`.

#### Scenario: Action joined to backend trace
- **WHEN** a RUM action `{ trace_id: "abc123", name: "click-checkout" }` is ingested and the same `trace_id` exists in `traces`
- **THEN** a SQL `SELECT * FROM rum_actions a JOIN traces t USING (trace_id)` returns the linked row pair

### Requirement: RUM Storage Quotas Honored

RUM ingest paths SHALL be subject to the same per-org quota enforcement as other ingest paths (`max_ingest_qps`, `max_storage_bytes`); replay archive size counts against `max_storage_bytes`.

#### Scenario: Replay rejected when over quota
- **WHEN** an org is over `max_storage_bytes` and posts a replay batch
- **THEN** the response is `413 Payload Too Large`, no object is written
