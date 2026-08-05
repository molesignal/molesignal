# Profiling Capability

## Purpose

`/debug/profile/{memory,cpu}` pprof 二进制端点，分别基于 jemalloc heap profiler 与 tikv `pprof` cpu sampler；Admin 权限保护。

## Requirements

### Requirement: pprof profile endpoints

The system SHALL expose `GET /debug/profile/cpu?seconds=<n>` and `GET /debug/profile/heap`. CPU profile SHALL capture for `n` seconds (default 30, max 120) and return pprof binary; heap profile SHALL return current jemalloc allocations as pprof binary.

#### Scenario: CPU profile produces pprof bytes

- **WHEN** a user GETs `/debug/profile/cpu?seconds=10` from localhost with profiling enabled
- **THEN** the response Content-Type is `application/octet-stream` and body decodes as a valid pprof `Profile` proto

### Requirement: Production safety gate

Profiling endpoints SHALL be disabled by default and accessible only when `MS_PROFILING_ENABLED=true`. When disabled, endpoints return 404. When enabled, endpoints SHALL refuse non-localhost requests unless `MS_PROFILING_ALLOW_REMOTE=true`.

#### Scenario: Disabled returns 404

- **WHEN** profiling not enabled and a user GETs `/debug/profile/heap`
- **THEN** the system returns 404 (no hint that the route exists)

#### Scenario: Remote denied by default

- **WHEN** profiling enabled but remote disallowed, and a request comes from a non-loopback address
- **THEN** the system returns 403 with body `{ "error": "profiling requires localhost" }`
