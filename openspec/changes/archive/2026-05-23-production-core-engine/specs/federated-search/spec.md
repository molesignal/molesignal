## ADDED Requirements

### Requirement: RemoteCluster Registry

The system SHALL maintain a `remote_clusters { id, org_id, name, advertise_addr, token_secret_ref, tls_verify, enabled, created_at, updated_at }` table and expose `GET/POST /api/v1/clusters`, `GET/PUT/DELETE /api/v1/clusters/:id` (Owner-only). `token_secret_ref` is a pointer (env var name OR `cipher_keys.id`) and is never serialized in responses.

#### Scenario: Owner registers remote cluster
- **WHEN** an Owner POSTs `{ name: "sf", advertise_addr: "https://obs-sf.example.com:5082", token_secret_ref: "env:OBS_SF_TOKEN", tls_verify: true }`
- **THEN** the response is `201 Created`; the secret value is NOT echoed back

#### Scenario: Disabled cluster excluded from search
- **WHEN** a cluster is marked `enabled = false`
- **THEN** federated searches that target it skip it without error

### Requirement: Federated Query Fan-Out

`POST /api/v1/query?clusters=<csv>` SHALL dispatch the query to each named remote cluster in parallel (using its `advertise_addr` and bearer-token credentials), reuse the same `arrow_flight` distributed querier protocol as in-cluster fan-out, and UNION the resulting `RecordBatch` streams locally before running final SQL aggregation. The default value of `clusters` is `"local"` only.

#### Scenario: Two-cluster fan-out
- **WHEN** the request includes `?clusters=local,sf` and both clusters are reachable
- **THEN** rows from both clusters are unioned, the response includes `meta.scanned_clusters = ["local", "sf"]`, and `final_aggregation` runs once locally

#### Scenario: Partial degradation
- **WHEN** `?clusters=local,sf,nyc` is sent and `nyc` is unreachable
- **THEN** the response is `200 OK` with results from `local` + `sf`, plus `meta.degraded_clusters = ["nyc"]` and `meta.degraded_reason.nyc = "<error>"`

#### Scenario: Cross-cluster auth rejected
- **WHEN** the bearer token configured for remote `sf` is invalid
- **THEN** that cluster is treated as degraded (not a hard 500), `federated_search_auth_errors_total{cluster="sf"}` increments
