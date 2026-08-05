## ADDED Requirements

### Requirement: Source map upload

The system SHALL accept JS source maps via `POST /api/v1/sourcemaps` with multipart form `{ service: string, release: string, file: <sourcemap json> }`. Maps are stored in object_store at `sourcemaps/<org>/<service>/<release>/<filename>.map` and indexed in `sourcemaps` table.

#### Scenario: Upload and lookup

- **WHEN** a user POSTs a source map for `service=web-app, release=1.2.3`
- **THEN** the response carries `{ "id": "<ksuid>", "object_key": "sourcemaps/<org>/web-app/1.2.3/..." }`
- **AND** a subsequent stack trace ingest referencing `service=web-app, release=1.2.3` can be translated

### Requirement: RUM error stack trace translation

When a RUM error event is ingested with `{ stack_trace, service, release }`, the system SHALL look up the matching source map and project `original_stack` (file / line / column / function name) onto the event before persisting. Translation failure SHALL NOT block the ingest; original stack is kept and `sourcemap_translated: false` is set.

#### Scenario: Translation succeeds

- **WHEN** a RUM error has `stack_trace="at e (a.js:1:42)"` and a matching map exists
- **THEN** persisted event carries `original_stack` with `[{ file: "src/app.tsx", line: 18, col: 5, fn: "handleClick" }]`
