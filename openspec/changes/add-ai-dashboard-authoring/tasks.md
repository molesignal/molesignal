## 1. Canonical Dashboard Contracts

- [x] 1.1 Create `contracts/dashboard/` ownership/layout documentation and move the current Dashboard v2 schema into `model/v2.schema.json` with stable `$id`, `schemaVersion: { const: 2 }`, explicit extension points, and no hand-maintained duplicate schema constant.
- [x] 1.2 Define `authoring/v1.schema.json` for title/time/refresh/folder, typed variables, text blocks, semantic panel sizing, visualization choices, and discriminated PromQL/SQL/trace/profile queries with strict unknown-field handling.
- [x] 1.3 Define `visualizations/v1.json` with compiler-supported visualization types, option schema versions, default options, compatible query kinds/data shapes, units, reducers, and default panel dimensions.
- [x] 1.4 Add shared valid/invalid fixtures covering Dashboard v2, authoring v1, nested elements, typed queries, visualization compatibility, unknown fields, and unsupported versions.
- [x] 1.5 Add a focused Rust contract-validation module using a Draft 2020-12 JSON Schema evaluator, bounded issue collection, JSON Pointer paths, canonical JSON serialization, and SHA-256 schema/model hashes.
- [x] 1.6 Add frontend generated contract assets and Ajv 2020 validation so Web import/editor boundaries consume the canonical Dashboard schema instead of a second runtime schema definition.
- [x] 1.7 Add schema-derived TypeScript compatibility tests plus a deterministic generation/drift-check command that fails when canonical schemas, generated assets, visualization defaults, or shared fixtures diverge.

## 2. Dashboard Write-Boundary Validation

- [x] 2.1 Extract the existing Dashboard manual checks into a dedicated server-side semantic validator for recursive IDs, grid bounds, ref IDs, refresh combinations, variables, visualization/query compatibility, and configured size/query budgets.
- [x] 2.2 Compose canonical JSON Schema and semantic validation in `DashboardService` so native create and update reject invalid nested models before metadata/version/repository mutation.
- [x] 2.3 Preserve an existing Dashboard UID on update and return structured validation issues without incrementing version or changing audit timestamps after a failed update.
- [x] 2.4 Keep Grafana import extension fields round-trippable while enforcing normalized renderer/query safety invariants through a separate import-validation mode.
- [x] 2.5 Add focused service/API tests for valid current models, duplicate IDs, out-of-grid elements, unsupported visualization/query combinations, failed-update immutability, UID preservation, and Grafana extension compatibility.

## 3. Authoring Domain and Compiler

- [x] 3.1 Add `domain/dashboard/authoring/` types for `DashboardAuthoringSpec`, typed panel/query variants, validation issues/warnings, draft lifecycle, capability metadata, and repository ports using `serde(deny_unknown_fields)` where appropriate.
- [x] 3.2 Add a versioned visualization manifest loader that validates its own structure once and returns only compiler-supported capability combinations to application/tool callers.
- [x] 3.3 Implement deterministic ID/ref generation and the 24-column layout compiler for ordered `small|medium|wide|full` size hints without overlaps or out-of-grid positions.
- [x] 3.4 Implement authoring-to-Dashboard compilation for metadata, time/refresh settings, variables, text elements, typed queries, visualization defaults, units, reducers, legends, thresholds, and empty collections.
- [x] 3.5 Validate compiler output with the canonical Dashboard contract and semantic validator, then compute a canonical model hash and version metadata.
- [x] 3.6 Add golden compiler tests for every supported visualization/query combination, deterministic layout/defaults, stable hashes, unsupported authoring versions, and invalid semantic combinations.
- [x] 3.7 Implement bounded query preflight/dry-run using trusted org context, stream/schema discovery, read-only parsing/planning, timeout/lookback/row/byte budgets, and empty-result warnings.

## 4. Dashboard Draft Persistence and Application Service

- [x] 4.1 Extend the existing initial migration with `intelligence_dashboard_drafts`, organization/creator/version/hash/status/TTL fields, JSONB spec/model payloads, indexes, and unique one-time-consumption constraints.
- [x] 4.2 Implement the Dashboard draft repository for org-scoped create/get, lazy expiry, ready-to-consumed transition, and consumed Dashboard lookup; add repository integration coverage.
- [x] 4.3 Implement `DashboardAuthoringService::capabilities` and `prepare` to run contract parsing, compilation, semantic validation, query preflight, warning aggregation, 30-minute bounded TTL, persistence, and preview metadata generation.
- [x] 4.4 Implement stale/hash/expiry/creator checks with stable `DRAFT_HASH_MISMATCH`, `DRAFT_EXPIRED`, `DRAFT_STALE`, and non-enumerating cross-org errors.
- [x] 4.5 Add an atomic `DashboardService` create-from-draft path that validates again, creates exactly one native Dashboard, consumes the draft in the same transaction, and returns the pre-existing result on replay.
- [x] 4.6 Wire authoring repositories/services through bootstrap/AppState without introducing API-to-infrastructure dependency inversions.
- [x] 4.7 Add application/integration tests for successful prepare, invalid query/no draft, empty-result warning, expiration, stale compiler revision, hash mismatch, concurrent consumption, and idempotent replay.

## 5. Mole Agent Dashboard Tools and Controlled Operation

- [x] 5.1 Register `get_dashboard_capabilities`, `prepare_dashboard`, and `propose_dashboard_creation` in `BuiltinToolKind`, presentation metadata, risk/access classification, and provider input schemas sourced from canonical contracts.
- [x] 5.2 Dispatch capability/preparation tools through `DashboardAuthoringService` with `intelligence.use`, `dashboards.create`, query permissions, Toolset/Profile filtering, trusted org/user context, and existing tool-call audit recording.
- [x] 5.3 Implement `propose_dashboard_creation` so it accepts only draft ID/hash/reason/impact, validates the referenced draft, respects chat execution policy, and creates a registered `create_dashboard` operation without persisting a Dashboard.
- [x] 5.4 Extend operation policy resolution with a Confirmation hard floor for Dashboard creation and map effective Confirmation/SingleApproval/DualApproval modes to zero/one/two required reviewers while allowing policy only to tighten.
- [x] 5.5 Refactor operation target loading/execution so alert actions retain current behavior and `create_dashboard` revalidates the draft/folder/permission before calling the atomic create-from-draft path.
- [x] 5.6 Permit the requestor to execute a Confirmation-mode proposal with `intelligence.use` plus `dashboards.create`, while preserving `intelligence.approve` and reviewer-count requirements for single/dual approval.
- [x] 5.7 Emit exactly-once federation CUD, activity audit, execution verification, Dashboard route/ID, and draft-consumption evidence for successful operations; redact compiled models from audit summaries.
- [x] 5.8 Add dispatcher/control integration tests for Advice-only/Read-only blocking, Profile/Toolset disablement, confirmation, tightened approval policies, cross-org drafts/folders, expired drafts, duplicate execution keys, and concurrent different keys.

## 6. Dashboard Skill Activation and Provider Tool Choice

- [x] 6.1 Add the versioned `dashboard-authoring` capability manifest and immutable `dashboard.authoring.v1` instruction asset/prompt with triggers, negative examples, contract range, required/optional tools, workflow, repair limit, and preview-before-propose rule.
- [x] 6.2 Extend prompt purposes and request DTOs with `dashboard_authoring`/`capability`, seed the built-in prompt in the initial migration, and retain prompt version/hash audit behavior for org overrides.
- [x] 6.3 Implement capability resolution precedence for explicit capability, `analysis_mode = dashboard`, Dashboard starter, and conservative high-confidence free-text routing without changing ordinary investigation prompts.
- [x] 6.4 Validate skill/tool/contract compatibility before provider invocation, supporting prepare-only degradation when proposal is disabled and fail-closed behavior when contract versions do not overlap.
- [x] 6.5 Add provider-neutral `ToolChoice::{Auto,None,Required,Specific}` to completion requests and validate that a specific tool exists in the filtered advertised schema.
- [x] 6.6 Map tool choice to OpenAI, OpenAI-compatible, and Anthropic request formats and add adapter request-body tests for all modes.
- [x] 6.7 Update Agent loop sync/stream paths to apply a specific initial choice once, return to Auto after the forced call, and surface unsupported provider capability without silent downgrade.
- [x] 6.8 Force `prepare_dashboard` only after Dashboard intent and input completeness checks; otherwise let the Dashboard skill ask for missing topic/data/time information.
- [x] 6.9 Add mocked-provider chat integration tests for explicit starter activation, free-text activation, forced prepare, repair after structured validation errors, preview-only degradation, proposal creation, and normal-chat non-regression.

## 7. Standards-Compliant MCP Schema Validation

- [x] 7.1 Extend MCP tool persistence in the initial migration/domain/repository with canonical `schema_hash`, schema dialect, synchronization timestamp, and unavailable diagnostic metadata.
- [x] 7.2 Validate MCP input schemas during sync with byte/nesting/reference limits, local-reference-only policy, supported dialect normalization, canonical hashing, and fail-closed unavailable status for malformed/unsupported schemas.
- [x] 7.3 Replace the top-level MCP argument checker with the shared compiled JSON Schema validator and structured bounded issues before any remote `tools/call` request.
- [x] 7.4 Ensure chat advertisement and execution load the same persisted MCP tool schema/hash revision and require successful resynchronization after a remote schema change.
- [x] 7.5 Add MCP tests for nested required/enum/bounds/additionalProperties/composition/local `$ref`, malformed/oversized/remote-ref schemas, hash revision changes, and proof that invalid calls never reach the remote server.

## 8. Dashboard Draft Preview and Confirmation UI

- [x] 8.1 Add org-scoped draft read/preview APIs with TTL, creator/permission, non-enumerating tenant checks, normalized warnings/issues, hash, and operation state.
- [x] 8.2 Add frontend API types/hooks for Dashboard capabilities, draft preview, proposal confirmation/review state, execution, and resulting Dashboard route.
- [x] 8.3 Update the Build Dashboard starter to send explicit `capability = dashboard_authoring` and show tool progress without exposing internal tool/function names in the assistant answer.
- [x] 8.4 Add `/ai/dashboard-drafts/:id` in a dedicated module and render the persisted compiled model through the existing read-only `DashboardRenderer`, not a second preview implementation.
- [x] 8.5 Display expiry, dry-run time range, panel warnings and structured validation issues with retry guidance; do not inline or trust a model-returned compiled model.
- [x] 8.6 Add Confirm/Create controls wired to proposal/execution policy, reviewer state, idempotency key, disabled/expired states, success toast, and navigation to `/dashboards/{id}`.
- [x] 8.7 Add Chinese/English copy, responsive states, Tooltip/aria labels, icon-only direct-copy behavior if used, and project-compliant focus styling without ring/outline/shadow focus frames.
- [x] 8.8 Add frontend tests for starter payload, preview rendering, warnings/errors, expiry, confirmation/approval variants, duplicate clicks, success navigation, tenant-safe failures, and accessibility semantics.

## 9. End-to-End Verification and Rollout Safety

- [x] 9.1 Run the shared contract corpus through Rust and Web validators and verify all built-in Dashboard seed models plus representative stored-model migration fixtures pass current read/write expectations.
- [x] 9.2 Add end-to-end integration coverage for user intent → capability activation → stream discovery → prepare → preview → proposal → confirmation/approval → exactly-one Dashboard creation → GET/render route.
- [x] 9.3 Add license, IAM, tenant-isolation, quota, Toolset/Profile, risk-policy, audit-redaction, federation, stale-draft, and concurrent-idempotency regression coverage.
- [x] 9.4 Verify production implementation files remain within the 500-line rule and authoring/contract/tool/provider/frontend responsibilities stay in dedicated modules rather than new generic helper files.
- [x] 9.5 Run one final Rust verification round after all backend changes, then only rerun failed items after fixes; run focused frontend tests, typecheck, touched lint, contract drift check, and accessibility checks.
- [x] 9.6 Validate the OpenSpec change, review generated API/schema artifacts for accidental identity fields or secrets, and document enable/disable plus rollback behavior for the Dashboard authoring capability.
