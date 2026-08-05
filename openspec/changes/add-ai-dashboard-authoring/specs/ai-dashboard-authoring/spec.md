## ADDED Requirements

### Requirement: Versioned Dashboard Authoring Contract

The system SHALL expose a versioned `DashboardAuthoringSpec` contract whose supported version is independent from the persisted Dashboard model `schemaVersion`. The authoring contract SHALL accept semantic dashboard intent such as title, time range, panels, typed queries, visualization choices, units, thresholds, variables, and optional folder placement; it SHALL NOT require the model to generate server-owned IDs, audit fields, grid coordinates, query ref IDs, visualization schema versions, or default visualization options.

#### Scenario: Minimal authoring specification is compiled
- **WHEN** the model submits an `authoringVersion = 1` specification with a title and one valid PromQL panel
- **THEN** the server generates all server-owned fields and compiles a complete Dashboard model using the current supported Dashboard `schemaVersion`

#### Scenario: Unsupported authoring version is rejected
- **WHEN** the model submits an authoring contract version that the server does not support
- **THEN** preparation fails with an `UNSUPPORTED_AUTHORING_VERSION` issue that includes the supported versions and creates no draft

### Requirement: Dashboard Authoring Capability Discovery

The system SHALL register a read-only `get_dashboard_capabilities` tool that returns the supported authoring versions, Dashboard model version, query kinds, visualization types, visualization option versions, units, limits, and relevant tool workflow. The returned catalog SHALL be derived from the same versioned manifests used by the compiler rather than duplicated in prompt text.

#### Scenario: Agent discovers current visualization support
- **WHEN** an authorized Agent calls `get_dashboard_capabilities`
- **THEN** the result lists only visualization and query combinations supported by the currently deployed compiler

#### Scenario: Disabled tool is not advertised
- **WHEN** the active Agent Profile or Toolset disables Dashboard authoring tools
- **THEN** Dashboard authoring tools are absent from the provider tool schema and a fabricated call is rejected by the dispatcher

### Requirement: Dashboard Draft Preparation

The system SHALL register a read-only-risk `prepare_dashboard` tool that validates `DashboardAuthoringSpec`, compiles it into a Dashboard model, performs domain validation, validates referenced streams and fields, and dry-runs each executable panel query within configured limits. Preparation SHALL have no Dashboard creation side effect.

#### Scenario: Valid specification produces a previewable draft
- **WHEN** an authorized Agent prepares a specification whose panel queries pass validation and dry-run
- **THEN** the tool returns an organization-scoped `draft_id`, canonical model hash, expiration time, normalized summary, warnings, and preview metadata without creating a Dashboard row

#### Scenario: Invalid query produces repairable issues
- **WHEN** a panel references a missing stream, field, or metric
- **THEN** the tool returns `VALIDATION_FAILED` with issues containing stable codes, JSON Pointer paths, user-safe messages, and `retryable` flags, and persists no executable draft

#### Scenario: Empty query result is a warning
- **WHEN** a syntactically valid and authorized query returns no rows during the bounded dry-run
- **THEN** preparation may succeed but returns a warning identifying the panel and tested time range

### Requirement: Dashboard Draft Integrity and Lifecycle

The system SHALL persist valid Dashboard drafts with `org_id`, creator, authoring contract version, normalized specification, compiled model, canonical model hash, status, creation time, and expiration time. Draft lookup and mutation SHALL be organization-scoped, expired drafts SHALL NOT be executable, and consumed drafts SHALL NOT create a second Dashboard.

#### Scenario: Model content is changed after preview
- **WHEN** a creation proposal supplies an expected hash that differs from the persisted draft hash
- **THEN** the system rejects the proposal with `DRAFT_HASH_MISMATCH` and does not create an approval or Dashboard

#### Scenario: Draft expires before execution
- **WHEN** an approved Dashboard creation operation targets a draft whose TTL has elapsed
- **THEN** execution fails closed with `DRAFT_EXPIRED` and creates no Dashboard

### Requirement: Controlled Dashboard Creation Operation

The system SHALL register a `propose_dashboard_creation` Agent tool and a `create_dashboard` operation. The tool SHALL accept only a draft reference, expected hash, reason, and impact; it SHALL create a confirmation/approval request according to the effective risk policy and SHALL NOT directly persist the Dashboard. Execution SHALL re-read and revalidate the draft, require `dashboards.create`, validate the target folder in the authenticated organization, call `DashboardService`, mark the draft consumed, and record the resulting Dashboard ID and route.

#### Scenario: Policy-mode chat proposes creation
- **WHEN** the active chat policy permits approval requests and the Agent proposes creation from a valid draft
- **THEN** the system creates a `create_dashboard` confirmation/approval record and the Dashboard remains absent until that operation is explicitly executed

#### Scenario: Read-only chat cannot propose creation
- **WHEN** the active chat uses Advice-only or Read-only execution policy
- **THEN** `propose_dashboard_creation` is rejected and neither an approval nor Dashboard is created

#### Scenario: Confirmed operation creates exactly one Dashboard
- **WHEN** an authorized user executes an approved `create_dashboard` operation twice with the same idempotency key
- **THEN** both requests return the same execution result and exactly one Dashboard row exists

### Requirement: Trusted Tenant and Actor Context

All Dashboard authoring tools and operations SHALL derive organization and actor identity exclusively from `ToolAuthContext`/`IamContext`. Model-supplied identity, approval, or organization fields SHALL be rejected or stripped, and draft, folder, stream, query, approval, and Dashboard access SHALL be checked against the authenticated organization.

#### Scenario: Model attempts cross-organization draft execution
- **WHEN** an Agent in organization B references a draft created in organization A
- **THEN** the system returns a non-enumerating not-found/forbidden result and does not reveal draft metadata or create a Dashboard

