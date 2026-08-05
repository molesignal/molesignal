## MODIFIED Requirements

### Requirement: pprof profile endpoints

When profiling is enabled, every service role SHALL expose node-local pprof-style endpoints `GET /debug/pprof/profile?seconds=<n>` and `GET /debug/pprof/heap` on the profiling listener. The existing `GET /api/v1/debug/profile/cpu?seconds=<n>` and `GET /api/v1/debug/profile/heap` endpoints SHALL remain as compatibility aliases with identical capture and response behavior. CPU profiling SHALL capture for `n` seconds (default 30, range 1 through 120) and return a valid pprof `Profile` protobuf. On supported jemalloc builds, heap profiling SHALL return a canonical pprof representation of current sampled allocations. Unsupported profile kinds SHALL return `501 Not Implemented` with a machine-readable reason rather than placeholder data.

#### Scenario: CPU profile produces pprof bytes

- **WHEN** a caller GETs `/debug/pprof/profile?seconds=10` from an allowed address with profiling enabled
- **THEN** the response is `200 OK` with `Content-Type: application/octet-stream`
- **AND** the body decodes as a valid pprof `Profile` containing CPU samples

#### Scenario: Compatibility CPU path uses the same capture

- **WHEN** a caller GETs `/api/v1/debug/profile/cpu?seconds=10`
- **THEN** validation, concurrency control, capture format, and response headers match `/debug/pprof/profile?seconds=10`

#### Scenario: Heap profile is normalized on a supported build

- **WHEN** a caller GETs `/debug/pprof/heap` on a supported jemalloc build after sampled allocations exist
- **THEN** the response body decodes as a valid pprof `Profile`
- **AND** the profile contains allocation sample types and stack locations

#### Scenario: Unsupported heap profiler is explicit

- **WHEN** a caller GETs `/debug/pprof/heap` on an unsupported platform or allocator
- **THEN** the response is `501 Not Implemented`
- **AND** the body names the unavailable heap profiling capability

#### Scenario: Concurrent CPU capture is rejected

- **WHEN** a CPU capture is already active and another CPU capture is requested
- **THEN** the second request returns `409 Conflict` with `Retry-After`
- **AND** it does not start a second sampler

### Requirement: Production safety gate

Profiling listeners and endpoints SHALL be disabled by default. Enabling SHALL be configurable through `[profiling]` settings, with `MS_PROFILING_ENABLED` and `MS_PROFILING_ALLOW_REMOTE` retained as compatibility overrides. The default bind address SHALL be loopback. When remote access is disabled, non-loopback requests SHALL receive `403`; when remote access is enabled, non-loopback requests SHALL additionally require an authenticated Administrator. Disabled endpoints SHALL return `404` and SHALL NOT initialize CPU or heap samplers.

#### Scenario: Disabled returns 404

- **WHEN** profiling is not enabled and a caller GETs `/debug/pprof/heap`
- **THEN** the system returns `404 Not Found`
- **AND** no heap profiling state is activated

#### Scenario: Remote denied by default

- **WHEN** profiling is enabled but remote access is disabled and a request comes from a non-loopback address
- **THEN** the system returns `403 Forbidden` with a machine-readable `profiling requires localhost` error

#### Scenario: Remote access still requires administrator authorization

- **WHEN** profiling and remote access are enabled and a non-loopback request lacks Administrator authorization
- **THEN** the system returns `401 Unauthorized` or `403 Forbidden`
- **AND** no capture starts

