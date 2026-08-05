## ADDED Requirements

### Requirement: Functions HTTP CRUD

The system SHALL expose `/api/v1/functions` for create / list / get / update / delete of user-defined functions (UDFs). Each function carries `{ name, language: vrl|js, body, params }` and is org-scoped.

#### Scenario: Create VRL function compiles on accept

- **WHEN** a user POSTs a function with `language: "vrl"` and a valid VRL body
- **THEN** the system SHALL compile via `vrl::compiler::compile` synchronously and persist; compilation errors return 400 with the error position

#### Scenario: JS function rejected when feature off

- **WHEN** a user POSTs a function with `language: "js"` and the build was made without `feature = "js"`
- **THEN** the system SHALL return 400 with message "javascript runtime not enabled in this build"

### Requirement: Function execution in ingest path

When a stream has functions bound via a pipeline, the system SHALL invoke each function in declared order on every `RawEvent` during ingest, before schema validation. A function returning an error SHALL push the event to `IngestResult.rejected` with the error message; processing of remaining events SHALL continue.

#### Scenario: One bad event does not block the batch

- **WHEN** a batch of 100 events is ingested and one event causes a VRL runtime error
- **THEN** the system SHALL accept 99 events and return `rejected: [{ index: <i>, reason: "<vrl err>" }]`
