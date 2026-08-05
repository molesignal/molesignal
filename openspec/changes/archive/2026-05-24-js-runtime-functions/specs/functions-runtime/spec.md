## ADDED Requirements

### Requirement: JavaScript Function Runtime

When built with `--features js-runtime` AND `[functions].js_runtime_enabled = true` at runtime, the system SHALL execute `Function { language: Js, source }` against each event in a V8 isolate (`deno_core`) with the following surface:

- A pre-installed global `molesignal` object exposing:
  - `fields`: mutable JS object reflecting the event's current fields (set / delete mutates the underlying event)
  - `set(name, value)`：shorthand for `fields[name] = value`
  - `del(name)`：deletes a field
  - `now()`：returns microseconds since epoch as a JS number
  - `log(level, msg)`：emits a tracing log
  - `parse_json(str)` / `encode_json(value)`：JSON helpers
  - `sha256(input)`：lowercase hex digest
- Per-function compile cache keyed on `(function_id, updated_at_micros)`，与 VRL 等价。
- Wall-clock budget 50ms per event; heap budget 32 MiB per isolate.

#### Scenario: JS function mutates event fields

- **WHEN** function source is `molesignal.set("level", molesignal.fields.severity.toLowerCase());` and event has `severity: "INFO"`
- **THEN** after execution the event has `level: "info"` AND `severity: "INFO"` (set, not replace)

#### Scenario: JS syntax error caught at precheck

- **WHEN** POST `/api/v1/functions` with `language: "js"`, `source: "function( bad"`
- **THEN** response is 400 with body containing `js syntax error: ...` (no DB write occurred)

#### Scenario: Execution timeout drops single event

- **WHEN** a JS function source contains an infinite `while(true) {}` loop and executes against event #5 in a batch of 10
- **THEN** event #5 is dropped with `IngestError { index: 5, reason: "js timeout 50ms" }`
- **AND** events 0-4 + 6-9 are processed normally
- **AND** the V8 isolate is recycled (not poisoned for next event)

#### Scenario: Memory limit aborts event

- **WHEN** JS function allocates > 32 MiB inside one event (e.g. `new Array(50_000_000)`)
- **THEN** isolate aborts with `IngestError { reason: "js heap exhausted" }` and processing continues

### Requirement: Runtime Feature Flag Gates JS

When `--features js-runtime` is NOT compiled in OR `[functions].js_runtime_enabled = false`，the system SHALL:
- Reject HTTP POST of `language: "js"` functions with 400 and message `javascript runtime not enabled` (existing behavior preserved).
- Return `IngestError { reason: "javascript runtime disabled" }` for affected events when the ingest pipeline encounters a JS function row (e.g. installed before the flag was disabled), without dropping the persisted row.

#### Scenario: Feature off rejects JS function POST

- **WHEN** binary built without `--features js-runtime` AND user POSTs JS function
- **THEN** 400 with `javascript runtime not enabled (build with --features js-runtime)`

#### Scenario: Toggle off at runtime stops execution but preserves rows

- **WHEN** binary built with `--features js-runtime` AND a JS function row already exists in DB AND ops sets `js_runtime_enabled = false`
- **THEN** the row is preserved (no auto-delete)，但 pipeline 跑到该 step 时 event 报 `IngestError { reason: "javascript runtime disabled" }`，不会 panic
