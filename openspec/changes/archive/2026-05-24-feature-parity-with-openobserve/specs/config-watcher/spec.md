## ADDED Requirements

### Requirement: TOML config file watch

The system SHALL watch the active config file via `notify` crate. When the file changes, the system SHALL re-parse it, diff against the running config, and apply hot-reloadable fields. Non-hot-reloadable field changes SHALL be logged as warnings and NOT applied; user must restart.

#### Scenario: Log level change applies live

- **WHEN** the user edits `[telemetry] log_level = "debug"` in the running config file
- **THEN** within 5 seconds, log output switches to debug level without restart

### Requirement: Immutable field guard

Fields including `[auth.jwt_secret]`, `MS_MASTER_KEY`, `[store.meta.dsn]`, `[wal.dir]` SHALL be marked immutable. Changes to these on disk SHALL emit a warn log `"immutable field <X> changed; restart required to apply"` and the in-memory value SHALL NOT change.

#### Scenario: Master key change refused

- **WHEN** `MS_MASTER_KEY` env or config equivalent is changed at runtime
- **THEN** the watcher logs a warn and the in-process master key remains the original (so existing cipher keys still open)
