# continuous-profiling Specification

## Purpose
TBD - created by archiving change add-continuous-profiling. Update Purpose after archive.
## Requirements
### Requirement: Profile Ingestion Protocols

The system SHALL accept continuous-profiling data through three ingress paths and normalize every accepted payload to a single internal representation before storage: (a) OTLP Profiles via `POST /api/v1/profiles/otlp`, (b) a Pyroscope-compatible `POST /api/v1/profiles/ingest?name=&from=&until=&format=`, and (c) raw pprof/JFR upload via `POST /api/v1/profiles/upload`. Unsupported `format` or undecodable payloads SHALL return `400` with a machine-readable reason.

#### Scenario: pprof upload accepted

- **WHEN** an authenticated client POSTs a gzip-compressed pprof `Profile` to `/api/v1/profiles/upload` with `service` and `profile_type`
- **THEN** the response is `202 Accepted`
- **AND** exactly one profile is persisted for that org

#### Scenario: Pyroscope ingest extracts service and labels

- **WHEN** a client POSTs a `format=pprof` body to `/api/v1/profiles/ingest?name=checkout%7Benv%3Dprod%7D&from=..&until=..`
- **THEN** the profile is persisted with `service=checkout` and label `env=prod`

#### Scenario: OTLP profiles enabled by default behind pinned adapter

- **WHEN** a client exports an OTLP profiles request to `/api/v1/profiles/otlp`
- **THEN** each contained profile is normalized and persisted
- **AND** the endpoint is enabled by default with no opt-in flag required
- **AND** decoding uses a pinned OTLP profiles proto version so Alpha-stage breaking changes stay contained in the adapter
- **AND** an optional config switch exists only to disable the endpoint for emergency rollback

#### Scenario: Unknown format rejected

- **WHEN** a client POSTs `/api/v1/profiles/ingest?format=unknown`
- **THEN** the response is `400` with body naming the unsupported format

### Requirement: Profile Normalization

The system SHALL convert pprof, Pyroscope `folded`/`lines`, and OTLP profiles inputs into a normalized profile carrying `service`, `profile_type`, `sample_types`, weighted stack `samples`, time window, profile-level labels, and optional `trace_id`/`span_id`. pprof input SHALL round-trip to the archived pprof form without loss of sample values or stack frames.

#### Scenario: pprof round-trips without loss

- **WHEN** a pprof profile with N samples is ingested and later downloaded
- **THEN** the downloaded pprof decodes to the same N samples with identical stack frames and values

#### Scenario: Folded text parsed

- **WHEN** a `format=folded` body with lines `a;b;c 10` is ingested
- **THEN** the normalized profile contains one sample with stack `[a,b,c]` and value `10`

#### Scenario: JFR recording parsed

- **WHEN** a Java client uploads a JFR recording to `/api/v1/profiles/upload`
- **THEN** the recording is parsed into one or more normalized profiles and persisted

### Requirement: Profile Archival and Metadata

When a profile is ingested, the system SHALL (1) write the normalized profile as a zstd-compressed pprof object at object_store key `profiles/<org_id>/<service>/<profile_type>/<yyyymmdd>/<profile_id>.pprof.zst`, and (2) append one metadata row to the `StreamType::Profiles` stream with `timestamp, service, profile_type, duration_nanos, sample_count, total_value, labels, trace_id, span_id, object_key, unsymbolized`.

#### Scenario: Ingest writes blob and metadata row

- **WHEN** a profile for `service=api`, `profile_type=cpu` is ingested
- **THEN** an object exists under `profiles/<org>/api/cpu/<date>/<id>.pprof.zst`
- **AND** a row referencing that `object_key` exists in the profiles stream with `total_value >= 0`

#### Scenario: Unsymbolized frames flagged

- **WHEN** an ingested profile contains frames with only `build_id`+address and no function name
- **THEN** the metadata row has `unsymbolized = true`

### Requirement: Flamegraph Aggregation Query

`GET /api/v1/profiles/flamegraph?service=&type=&from=&to=&label=k:v` SHALL select matching profiles via the metadata stream, merge their stacks into an aggregated tree summed by frame path, and return a flamebearer structure (`names[]` + `levels[]`). The number of merged profiles SHALL be capped (default `profiles.flamegraph.max_merge = 1000`); over-capacity responses SHALL sample evenly across the window and set `truncated: true`.

#### Scenario: Window merge returns flamebearer

- **WHEN** a Viewer GETs `?service=api&type=cpu&from=now-1h&to=now`
- **THEN** the response is `200 OK` with a `flamebearer` containing `names` and `levels`
- **AND** the root level total equals the sum of merged sample values

#### Scenario: Over-limit query is truncated, not failed

- **WHEN** the window matches more profiles than `max_merge`
- **THEN** the response is `200 OK` with `truncated: true` and a representative sampled merge

### Requirement: Differential Flamegraph

`GET /api/v1/profiles/diff?service=&type=&from=&to=&baselineFrom=&baselineTo=` SHALL aggregate a baseline window and a comparison window and return a flamebearer whose per-frame values carry the signed delta (comparison minus baseline).

#### Scenario: Diff returns per-frame deltas

- **WHEN** a function's cost grows between baseline and comparison windows
- **THEN** the diff flamebearer marks that frame with a positive delta
- **AND** a function present only in baseline is marked with a negative delta

### Requirement: Trace-to-Profile Correlation

When a profile or its samples carry `trace_id`/`span_id`, the system SHALL persist them on the metadata row so callers can retrieve the flamegraph for a given trace/span window.

#### Scenario: Span profile lookup

- **WHEN** a profile tagged `trace_id=abc, span_id=s1` is ingested and a caller GETs `/api/v1/profiles/flamegraph?trace_id=abc&span_id=s1`
- **THEN** the returned flamebearer aggregates only samples linked to that span

### Requirement: Profile Listing and Download

`GET /api/v1/profiles?service=&type=&from=&to=&label=` SHALL list profile metadata (no stack bodies); `GET /api/v1/profiles/{id}` SHALL stream the archived pprof object.

#### Scenario: List then download

- **WHEN** a Viewer lists profiles for `service=api` then GETs one by `id`
- **THEN** the list returns rows with `id, service, profile_type, timestamp, total_value`
- **AND** the download responds with the pprof object and `Content-Type: application/octet-stream`

### Requirement: Edition Gating for Enhanced Profiling

Core profiling SHALL be available in OSS: all three ingest protocols, archival + metadata storage, single-window flamegraph browsing, listing/filtering, trace correlation, and default retention. Enhanced capabilities SHALL be license-gated: differential flamegraph, cross-service/large-window aggregation, long-term retention, server-side symbolication, and a Pyroscope `/render` compatibility egress.

#### Scenario: OSS single-window flamegraph allowed

- **WHEN** an OSS org requests `/api/v1/profiles/flamegraph` for a single service window
- **THEN** the response is `200 OK`

#### Scenario: Diff gated without enhanced license

- **WHEN** an org without the profiling-enhanced license requests `/api/v1/profiles/diff`
- **THEN** the response indicates the feature is license-gated and names the required edition
- **AND** the body is not a bare 403 payload

