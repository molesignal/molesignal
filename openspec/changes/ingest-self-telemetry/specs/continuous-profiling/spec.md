## ADDED Requirements

### Requirement: Selectable Profile Metadata Stream

The continuous-profiling storage service SHALL accept an explicit metadata stream name from trusted internal callers while retaining `default` for existing public upload, Pyroscope, and OTLP profile ingestion. Self-profile captures SHALL use `_molesignal`, SHALL archive canonical pprof bytes through the existing object-store path, and SHALL write metadata to `(management_org, "_molesignal", Profiles)`. The stream selector SHALL NOT be accepted from public request parameters or headers.

#### Scenario: Self profile uses the system stream

- **WHEN** the self-telemetry runtime stores a CPU profile
- **THEN** the profile blob is archived through the normal profile object-store path
- **AND** its metadata is written to `profiles/_molesignal`
- **AND** no metadata row is written to `profiles/default`

#### Scenario: Public profile ingestion remains on default

- **WHEN** an authenticated client uploads a pprof through the existing public endpoint
- **THEN** its metadata remains in the `default` profiles stream
- **AND** the client cannot redirect it to `_molesignal`

### Requirement: On-Demand Self Profile Persistence

When self profile ingestion is enabled, each successful on-demand CPU or heap capture from a pprof-style endpoint SHALL be returned to the caller and asynchronously persisted through the same self-profile path. Persistence failure SHALL be observable but SHALL NOT corrupt or truncate an already completed HTTP response.

#### Scenario: pprof request also becomes queryable

- **WHEN** self profiles and the pprof listener are enabled and a CPU capture succeeds
- **THEN** the caller receives the complete pprof body
- **AND** a corresponding metadata row becomes queryable in `profiles/_molesignal`

#### Scenario: Persistence failure preserves the capture response

- **WHEN** a pprof capture succeeds but internal profile archival fails
- **THEN** the caller still receives the valid captured profile
- **AND** a self-telemetry persistence failure metric increases

