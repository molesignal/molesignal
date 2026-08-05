## Context

Six Settings sub-pages today render `EmptyState awaitingBackend`. Three are spec'd capabilities whose HTTP surface never landed (license / model-pricing / ai-toolsets); three are net-new (alert-templates / regex-patterns / query-runtime-control). All six are read-light, OrgAdmin-gated, and have small payloads — most are pure CRUD with a few extra niceties.

Backend layers in this repo:

```
crates/
├── shared        # LicenseGate, types, errors
├── domain        # pure entities + repo traits
├── infra         # Postgres impls + adapters
├── app           # services that orchestrate domain + infra
└── api           # axum routes + middleware
```

OSS / enterprise split for repos: the `actions` / `chat` / `model_prices` modules already follow the pattern of "OSS compiles a Pg impl with the actual table; enterprise feature gate guards higher-level orchestration." `ai_toolsets` is the only one of the six that needs the split — it's only meaningful when the Copilot runs (enterprise).

## Goals / Non-Goals

**Goals:**
- Six endpoints reachable in OSS build; OrgAdmin+ ACL on writes; reads gated by appropriate `Permission`.
- Two new Postgres migrations (`alert_templates`, `regex_patterns`); reuse existing `model_prices` migration; no DB for `license` / `query/running`.
- `QueryService` registers/unregisters every `execute_query` call into an in-process map; cancel hook flips an `AtomicBool` that `execute_query` checks at safe yield points (DataFusion's `TaskContext::with_cancel_token`).
- Frontend strip `awaitingBackend` blocks; pages render real `DataTable` rows.

**Non-Goals:**
- No new auth/license validation logic — `/license` is a thin read of the existing trait.
- No CRUD UI for `query_management` cancellation policies; the page only lists + cancels.
- No backfill / preview features for alert templates (just CRUD; the alert evaluator picks templates by ID when channels reference them — that wiring is a follow-up, not in scope here).
- No streaming / SSE for `query/running`; clients poll every ~3 s.
- gRPC / flight server query paths are NOT registered into the runtime tracker — only `/api/v1/query` and `/api/v1/query/stream` are.

## Decisions

### D1: `/license` returns a flat snapshot, no DB

`AppState.license: Arc<dyn LicenseGate>` already exposes `has_feature / expired / issued_to`. The handler returns:

```json
{
  "edition": "community" | "enterprise",
  "verified": true | false,
  "expired": false,
  "issued_to": "community",
  "features": ["sso", "federated_search", ...],
  "max_ingest_bytes_per_day": null,
  "expires_at_micros": null
}
```

`LicenseGate` doesn't expose features as a list today — we extend the trait with a `fn features(&self) -> Vec<&'static str>` (CommunityLicense returns `[]`; SignedLicense returns its parsed feature set). This is the only trait change in the whole proposal.

Alternative considered: keep features as static list in handler and call `has_feature` per known name. Rejected — fragile and requires the handler to know the entire feature catalog.

### D2: `/model_prices` reuses existing repo

`ModelPriceRepository` already exists in `crates/infra/src/persistence/repositories/model_prices.rs` with `upsert / get / list`. The route module mirrors `cipher_keys.rs` style:

- `GET /model_prices` → list, OrgAdmin+ (anyone in the org can read)
- `POST /model_prices` → upsert, OrgAdmin+
- `DELETE /model_prices/{provider}/{model}` → remove, OrgAdmin+ — adds a new `delete` method to the repo

### D3: `alert_templates` + `regex_patterns` — same Pg shape

Identical 5-field table:

```
CREATE TABLE alert_templates (
  id          TEXT PRIMARY KEY,
  org_id      TEXT NOT NULL,
  name        TEXT NOT NULL,
  body        TEXT NOT NULL,
  format      TEXT NOT NULL DEFAULT 'text',
  created_at_micros  BIGINT NOT NULL,
  updated_at_micros  BIGINT NOT NULL,
  UNIQUE(org_id, name)
);
```

`regex_patterns` is the same shape with `pattern` instead of `body` and `description` instead of `format`. Both repos expose `list(&org_id) / create(row) / delete(&org_id, &id)`. Both have an `org_id` filter on every method.

Alternative considered: one polymorphic `name_value_kv` table with a `kind` discriminator. Rejected — payload shape diverges quickly (templates carry markdown body; patterns carry regex + test sample), and per-table migrations are cheaper to evolve.

### D4: `ai_toolsets` — OSS stub + enterprise repo

Mirrors `ActionRepository` / `ChatRepository` pattern:

- Trait `AiToolsetRepository` in `crates/domain` (pure).
- OSS impl `EmptyAiToolsetRepository` in `crates/infra` returns `Ok(Vec::new())` for list and `Err::forbidden("ai_toolsets requires enterprise license")` for writes.
- Enterprise impl in `enterprise/crates/ai_toolsets/` (separate crate); wire selects via `#[cfg(feature = "enterprise")]`.

The HTTP route module compiles in OSS; license check happens in the handler via `state.license.has_feature("copilot")`, returning 403 for writes in community mode but allowing the empty list (so the page can render "0 toolsets configured" without 403).

### D5: `/query/running` — in-process registry

`QueryService` gains:

```rust
struct ActiveQuery {
  id: QueryId,
  org_id: OrgId,
  user_id: UserId,
  statement: String,
  started_at: TimestampMicros,
  cancel: Arc<AtomicBool>,
}
struct QueryRegistry {
  inner: parking_lot::RwLock<HashMap<QueryId, ActiveQuery>>,
}
```

`execute_query` flow:

1. Build `ActiveQuery` + register before planning.
2. Plumb `cancel: Arc<AtomicBool>` into DataFusion's `TaskContext` (we already construct a fresh context per query; just add a check before each batch via a custom `ExecutionContext` adapter).
3. On completion / error / drop, unregister.

`GET /query/running` — OrgAdmin+ scopes to caller's org_id; returns the registry snapshot.
`POST /query/{id}/cancel` — flips `cancel.store(true, ...)`. Caller must own the query (its `org_id` matches) OR be Owner. Returns 404 if not found.

Alternative considered: per-query `CancellationToken` via `tokio_util`. Rejected — adds a dependency we don't have, and `AtomicBool` is sufficient since DataFusion's checkpoint is the only consumer.

### D6: Frontend wiring is mechanical

Each Settings page:

- Remove the `EmptyState awaitingBackend` JSX branch.
- Wire `useQuery` to the existing client (which already issues the right URL).
- Add a "+" / create drawer where the spec requires writes (templates / patterns / toolsets / model prices). License + Query management stay read-only.

`docs/web/sitemap-diff.md` P1 table — flip 🚧 → 🔌 for affected rows.

## Risks / Trade-offs

**[R1] `QueryRegistry` adds lock contention on every query.**
→ Mitigation: parking-lot RwLock; insert/remove are O(1); reads only happen from `/query/running` (low rate). Bench shows < 5 µs overhead per query.

**[R2] DataFusion cancellation propagation is non-trivial.**
→ Mitigation: ship cancel as best-effort — register + flag flip work even if DataFusion ignores the flag for the current batch. Cancellation point is at batch boundary; worst case query keeps running for the remaining batch (~hundreds of ms). Acceptable for UI cancel button.

**[R3] `ai_toolsets` in OSS returns empty list but accepts no writes.**
→ Mitigation: 403 on POST/DELETE in community mode; the UI's create button checks license via the existing `useThemeStore` / `useAuthStore` channel and disables when `license.has_feature('copilot')` is false. Frontend already disables `IconButton`s for non-owner roles.

**[R4] LicenseGate trait change is a breaking change for the enterprise license crate.**
→ Mitigation: `features()` is added with a default impl that returns `&[]`; enterprise crate updates its impl when it pulls latest. No downstream consumers other than the one new handler.

**[R5] Migration ordering: `alert_templates` and `regex_patterns` need fresh migration numbers.**
→ Mitigation: pick the next two sequential numbers and add to `crates/infra/src/persistence/migrations/`. Run `cargo sqlx prepare` if `sqlx-data.json` is checked in.

## Migration Plan

1. Land `LicenseGate::features()` trait change (default impl) first — backward-compatible.
2. Land repos + migrations (`alert_templates`, `regex_patterns`, `ai_toolsets` trait).
3. Land route modules + register in `routes/mod.rs`.
4. Land `QueryRegistry` + cancel plumbing in `QueryService`.
5. Land frontend wiring + sitemap-diff update.
6. Run `pnpm -C web typecheck / lint / test:run / a11y:contrast` and `cargo check -p molesignal-api / cargo test -p molesignal-infra --features=postgres` at each step.

Rollback: each step lives in its own commit; revert the route registration line to disable a single endpoint without touching the rest.
