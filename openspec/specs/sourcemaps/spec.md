# RUM Debug Artifacts Capability

## Purpose

统一管理 Web Source Map、Flutter AOT symbols、Android R8/NDK symbols 与 Apple dSYM，并在 RUM error 入库前恢复原始函数和源码位置。

## Requirements

### Requirement: Debug artifact upload

The system SHALL accept debug artifacts via `POST /api/v1/debug-artifacts` with multipart fields `{ application_id, service, release, kind, platform, architecture?, debug_id?, file }`. Supported kinds are `javascript_sourcemap`, `flutter_symbols`, `android_mapping`, `android_native_symbols`, and `apple_dsym`. Metadata is tenant-scoped in `debug_artifacts`; bytes are stored under `debug-artifacts/<org>/...`.

The former `/api/v1/sourcemaps` compatibility route SHALL NOT be exposed.

#### Scenario: Upload and lookup

- **WHEN** a user uploads an artifact for `application_id=storefront, service=web-app, release=1.2.3`
- **THEN** the response carries its identity, kind, platform, checksum, size, and upload time
- **AND** a subsequent RUM error referencing the same application, service, release, platform, architecture, and debug ID can be translated

#### Scenario: Legacy route is removed

- **WHEN** a caller requests `GET /api/v1/sourcemaps`
- **THEN** the server returns `404 Not Found`

### Requirement: RUM error stack trace translation

When a RUM error event is ingested, the system SHALL look up a matching artifact inside the authenticated organization and application, then project `original_file`, `original_line`, `original_column`, `original_function`, and artifact identity onto translated frames before persisting. Translation failure SHALL NOT block ingest; original frames remain intact and the event records a `completed`, `partial`, or `missing` symbolication status.

#### Scenario: Translation succeeds

- **WHEN** a RUM error has `stack_trace="at e (a.js:1:42)"` and a matching map exists
- **THEN** the persisted frame carries `original_file`, `original_line`, `original_column`, and `original_function`
- **AND** the event records symbolication status plus the matching debug artifact ID and kind
