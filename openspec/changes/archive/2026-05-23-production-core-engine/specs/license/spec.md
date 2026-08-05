## ADDED Requirements

### Requirement: License File Parsing

On startup, the system SHALL look for `[license].file` (default `/etc/molesignal/license.json`). The file content is a JSON document `{ payload: { product, edition, max_ingest_bytes_per_day, max_users, expires_at, features: [String] }, signature_b64 }` signed by the project's Ed25519 root public key (compiled into the binary). Invalid signature SHALL fail `wire::build_state`. Absence of file SHALL run in "community" edition (no max limits, no `enterprise` features).

#### Scenario: Valid license accepted
- **WHEN** the file is present and signature verifies
- **THEN** `main()` starts; `/api/v1/system/license` (Admin+) returns the parsed payload with `verified: true`

#### Scenario: Tampered license rejected
- **WHEN** the file's `max_ingest_bytes_per_day` is edited but the signature is unchanged
- **THEN** `main()` exits with `Err("license signature invalid")` before any role starts

#### Scenario: Missing file falls back to community
- **WHEN** the file does not exist
- **THEN** `main()` starts in community mode; `/api/v1/system/license` returns `{ edition: "community", verified: false }`

### Requirement: Feature Gating and Quota Enforcement

The system SHALL refuse to enable `[auth.sso]`, `[federated_search]`, or LLM telemetry fan-out when the active license does not include the matching `features` entry; the offending feature SHALL log a warn and silently no-op. Daily ingest bytes are tracked in memory and persisted hourly to `license_usage_daily` table; exceeding `max_ingest_bytes_per_day` SHALL return `429 Too Many Requests` with `Retry-After: <secs_until_midnight_utc>` for further ingests that day.

#### Scenario: Enterprise feature blocked in community
- **WHEN** community mode is active and a request hits `/api/v1/auth/sso/login`
- **THEN** the response is `403 Forbidden` with `{ "error": "feature 'sso' requires enterprise license" }`

#### Scenario: Daily ingest cap reached
- **WHEN** the day's ingested bytes reach `max_ingest_bytes_per_day`
- **THEN** subsequent ingest requests return `429 Too Many Requests` with a `Retry-After` header counting to next UTC midnight; `license_ingest_cap_hits_total` increments

#### Scenario: Expired license downgrades
- **WHEN** `expires_at < now`
- **THEN** the server logs a warn at startup, runs as `community`, and the license endpoint returns `verified: true, expired: true`
