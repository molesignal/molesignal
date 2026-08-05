# RUM Capability

## Purpose

浏览器、Flutter、Android 原生和 iOS 原生 Real User Monitoring：接收 sessions、actions、
errors 与 replay，按应用隔离数据，在 error 入库前执行调试产物符号化，并通过 `trace_id`
关联后端 traces。uni-app 不在当前范围。

## Requirements

### Requirement: RUM JSON receivers

The system SHALL expose `POST /api/v1/rum/{sessions,actions,errors}` accepting one JSON
object or an array of JSON objects. Sessions, actions, and errors are ingested into the
`rum_sessions`, `rum_actions`, and `rum_errors` log streams respectively. Every receiver requires
`rum.write`.

#### Scenario: Mobile session start is accepted

- **WHEN** an authorized mobile SDK posts a session event carrying `application` and `session_id`
- **THEN** the server inserts the canonicalized event into `rum_sessions`
- **AND** client-supplied IP aliases are replaced with the trusted server-side client IP result

### Requirement: Application-bound public credentials

A request authenticated by an `msrum_` credential SHALL be restricted to the application stored on
that credential. A personal API token with `rum.write` may explicitly name an application.

#### Scenario: Application is filled from the credential

- **WHEN** an `msrum_` credential bound to `checkout-mobile` omits the application field
- **THEN** the persisted event uses `application=checkout-mobile`

#### Scenario: Cross-application payload is rejected

- **WHEN** the same credential submits `application=another-app`
- **THEN** the server returns `403 Forbidden` before ingest or object storage

### Requirement: Replay segment persistence

The system SHALL expose `POST /api/v1/rum/replay` accepting
`{ application?, session_id, seq, events }`. It validates bounded rrweb/timeline JSON events,
encodes them as NDJSON+zstd, and stores them beneath the authenticated organization and a hash of the
canonical application. Metadata uniqueness and idempotency are scoped by
`(org_id, application_id, session_id, seq)`.

#### Scenario: Replay events are archived idempotently

- **WHEN** an SDK posts a valid replay segment for `session_id=ses-123`, `seq=1`
- **THEN** the object key has the form
  `<org>/rum/<application-hash>/ses-123/0000000001-<content-hash>.ndjson.zst`
- **AND** repeating the same content returns the existing segment
- **AND** repeating the sequence with different content returns a conflict

#### Scenario: Replay bounds are enforced

- **WHEN** a segment exceeds 500 events or 8 MiB uncompressed, or its session would exceed 2,000
  segments or 100 MiB
- **THEN** the request is rejected before metadata insertion

### Requirement: Error symbolication before ingest

The errors receiver SHALL bind the application first, match a tenant/application/build-scoped debug
artifact, and append translated `original_*` fields before writing `rum_errors`. Missing or invalid
artifacts SHALL NOT reject the original error event.

#### Scenario: Missing symbols preserve the error

- **WHEN** an error frame has no unambiguous matching debug artifact
- **THEN** the original frame remains unchanged
- **AND** the event records symbolication status `missing` or `partial`

### Requirement: RUM session-to-trace correlation

When a RUM action carries `trace_id`, the system SHALL preserve it in `rum_actions` so queries can
join the action to backend traces.

#### Scenario: Action joins a backend trace

- **WHEN** a RUM action and a trace share `trace_id=abc123`
- **THEN** a query joining `rum_actions` and `traces` by `trace_id` returns the linked records
