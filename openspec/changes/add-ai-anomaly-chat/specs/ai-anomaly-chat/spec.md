## ADDED Requirements

### Requirement: Postgres-backed AI model providers

The system SHALL store AI model provider configuration in Postgres per org, including provider type, display name, base URL, default model, enabled state, timeout, token limits, and masked key metadata. API keys MUST be encrypted with `CipherRootKey` before persistence and MUST NOT appear in API responses, logs, audit payloads, chat messages, or archives.

#### Scenario: Admin creates provider with encrypted key

- **WHEN** an Admin POSTs `/api/v1/ai/providers` with provider metadata and an API key
- **THEN** the provider row is persisted
- **AND** the API key is stored only as encrypted ciphertext plus nonce
- **AND** the response includes masked key metadata but not the plaintext key

#### Scenario: Key rotation is audited

- **WHEN** an Admin POSTs `/api/v1/ai/providers/{id}/rotate_key`
- **THEN** the old encrypted key is replaced or retired according to repository policy
- **AND** an audit event with `action = "ai.provider.rotate_key"` is recorded without plaintext secret material

### Requirement: Built-in prompt templates with editable scoped overrides

The system SHALL provide built-in default prompt templates in Postgres for at least these stable builtin keys: `system.default`, `analysis.anomaly`, `analysis.root_cause`, `alert.explain`, and `query.generate`. Built-in rows SHALL be immutable. Admins SHALL be able to create org-scoped overrides, and users with prompt-write permission SHALL be able to create user-scoped overrides. Every override SHALL retain a parent/builtin reference and version.

#### Scenario: Fresh org has default prompts

- **WHEN** an org has never customized AI prompts
- **THEN** `GET /api/v1/ai/prompts` returns the built-in prompt templates as available defaults
- **AND** anomaly chat can run without any manual prompt setup

#### Scenario: User customizes built-in prompt

- **WHEN** a user edits the built-in `analysis.root_cause` prompt from the UI
- **THEN** the backend creates a scoped override instead of mutating the built-in row
- **AND** the override records `parent_id`, `builtin_key`, incremented `version`, and `updated_by`

### Requirement: Prompt resolution and render traceability

For every AI chat request, the system SHALL resolve a prompt by explicit `prompt_template_id`, then user default, then org default, then built-in default for the request purpose. Prompt rendering SHALL accept only variables allowed by the template's `variables_schema`. The selected template id, builtin key, version, and SHA-256 hash of the rendered prompt SHALL be persisted with the relevant chat message and audit metadata.

#### Scenario: Explicit prompt wins

- **WHEN** a chat message request includes `prompt_template_id = P`
- **THEN** the backend uses prompt P if the caller can read it
- **AND** the resulting chat message stores P's id, version, builtin key, and rendered prompt hash

#### Scenario: Unknown render variable is rejected

- **WHEN** a prompt template body references a variable not allowed by its `variables_schema`
- **THEN** creating or updating the template returns `400 Bad Request`

### Requirement: Time-range anomaly chat workflow

The system SHALL expose an AI chat workflow where a user can ask a natural-language question scoped to a time range and optional streams. The backend SHALL pass only sanitized context and tool schemas to the model. Tenant data retrieval SHALL happen through backend tools that derive org/user identity from auth context.

#### Scenario: User asks for anomaly analysis

- **WHEN** a user asks "what changed between 10:00 and 11:00" with a time range
- **THEN** the backend creates or updates a chat session
- **AND** the model may call backend tools for logs, metrics, traces, streams, or alerts
- **AND** the final answer contains summary, anomaly points, evidence, likely causes, suggested next steps, related links, and confidence

#### Scenario: Model-supplied org id is ignored

- **WHEN** the model emits a tool call whose arguments include `org_id = "other-org"`
- **THEN** the backend tool executes using the authenticated org from `AuthContext`
- **AND** no data from `other-org` is queried or returned

### Requirement: Tool evidence capture and object-store spillover

Every AI tool call SHALL persist compact evidence metadata in Postgres and SHALL spill large raw tool results to object storage. Evidence metadata SHALL include tool name, sanitized arguments, status, latency, row count or scanned row count when available, and object key when spillover occurs.

#### Scenario: Large tool result spills to object storage

- **WHEN** a tool result exceeds the configured inline byte limit
- **THEN** the raw result is written under `ai-chat/{org_id}/{session_id}/tool-results/...`
- **AND** the chat message stores only the evidence summary and object key

### Requirement: AI chat transcript archive

The system SHALL archive complete AI chat transcripts to object storage with metadata retained in Postgres. A transcript archive SHALL include session metadata, messages, prompt references, rendered prompt hashes, tool-call summaries, evidence object keys, token usage, model cost, and archive checksum. Deleting a chat session SHALL soft-delete metadata and preserve archive records until retention removes them.

#### Scenario: Completed session archived

- **WHEN** an AI chat session is closed or selected for archive
- **THEN** the backend writes a transcript JSON object under the AI archive prefix
- **AND** stores archive object key, SHA-256, byte size, status, and timestamp in Postgres
- **AND** records an `ai.chat.archive` audit event

#### Scenario: Archive failure does not erase history

- **WHEN** object storage write fails during archive
- **THEN** chat session and message rows remain available in Postgres
- **AND** archive status records the failure
- **AND** an audit event records the failed archive attempt without raw transcript content
