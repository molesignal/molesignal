# Regex Patterns Capability

## Purpose

Named, org-scoped regex pattern storage with byte-exact round-trip. Patterns are referenceable from VRL functions through a future `lookup_pattern(name)` builtin.

## Requirements

### Requirement: Regex Pattern CRUD

The system SHALL expose `GET /api/v1/regex_patterns`, `POST /api/v1/regex_patterns`, and `DELETE /api/v1/regex_patterns/{id}` backed by a `RegexPatternRepository` over a Postgres `regex_patterns` table with columns `(id TEXT PK, org_id TEXT, name TEXT, pattern TEXT, description TEXT, created_at_micros BIGINT, updated_at_micros BIGINT, UNIQUE(org_id, name))`. All routes require `OrgAdmin+`; reads are scoped to the caller's `org_id`.

#### Scenario: List returns this org only

- **WHEN** an OrgAdmin GETs `/api/v1/regex_patterns`
- **THEN** the response is a JSON array of every pattern whose `org_id` matches the caller's org

#### Scenario: Create validates regex syntax

- **WHEN** an Admin POSTs `{ name: "ip", pattern: "[0-9]{1,3}\\.[0-9]{1,3}", description: "ipv4 prefix" }`
- **THEN** the row persists
- **AND** subsequent GET returns it

#### Scenario: Invalid regex rejected

- **WHEN** an Admin POSTs a pattern that fails `regex::Regex::new` compilation
- **THEN** the response is `400 Bad Request` with the underlying compile error message
- **AND** no row is inserted

### Requirement: Pattern Reuse Hook

Stored patterns SHALL be referenceable from VRL functions through a future `lookup_pattern(name)` builtin. This change DOES NOT implement the builtin — only the storage + management surface. The pattern body MUST round-trip exactly as written.

#### Scenario: Pattern round-trips byte-for-byte

- **WHEN** an Admin creates a pattern with body `(?i)^bearer\\s+([a-z0-9_\\-\\.]+)$`
- **AND** another caller GETs the list
- **THEN** the returned pattern body is byte-identical to the input
