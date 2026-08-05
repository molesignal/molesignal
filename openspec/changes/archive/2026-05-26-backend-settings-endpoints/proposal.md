## Why

`web-feature-parity-settings` shipped 16 Settings sub-pages but six of them render an `EmptyState awaitingBackend` because the corresponding endpoints don't exist:

| Sub-page | Endpoint frontend expects |
| --- | --- |
| `/settings/license` | `GET /api/v1/license` |
| `/settings/model_pricing` | `GET/POST /api/v1/model_prices` (already spec'd, never landed) |
| `/settings/alert_templates` | `GET/POST/DELETE /api/v1/alerts/templates` |
| `/settings/regex_patterns` | `GET/POST/DELETE /api/v1/regex_patterns` |
| `/settings/ai_toolsets` | `GET/POST/DELETE /api/v1/ai_toolsets` |
| `/settings/query_management` | `GET /api/v1/query/running` + `POST /api/v1/query/{id}/cancel` |

Three of these (license, model-pricing, ai-toolsets) already have spec'd capabilities; the others are net-new. Closing the gap removes the "awaiting backend" banner from every Settings page and unlocks the user-facing CRUD that the spec already promises.

## What Changes

### New endpoints

- `GET /api/v1/license` — read-only view of the active `LicenseGate` impl (edition / verified / expired / features / issued_to / max_ingest_bytes_per_day). No DB.
- `GET / POST /api/v1/model_prices` + `DELETE /api/v1/model_prices/{provider}/{model}` — list + upsert, reusing the existing `ModelPriceRepository`.
- `GET / POST /api/v1/alerts/templates` + `DELETE /api/v1/alerts/templates/{id}` — reusable message templates that alert channels can reference.
- `GET / POST /api/v1/regex_patterns` + `DELETE /api/v1/regex_patterns/{id}` — VRL regex pattern shortcuts.
- `GET / POST /api/v1/ai_toolsets` + `DELETE /api/v1/ai_toolsets/{id}` — Copilot tool definitions. OSS surface compiles (returns `[]` from an in-memory stub); enterprise build uses a Pg repo.
- `GET /api/v1/query/running` + `POST /api/v1/query/{id}/cancel` — list in-flight queries with cancellation. Backed by an in-process registry attached to `QueryService`.

### Frontend wire-up

- Replace `EmptyState awaitingBackend` in the six Settings sub-pages with real data lists fed by the new clients (`api/license.ts`, `api/regexPatterns.ts`, `api/aiToolsets.ts`; existing `runningQueries.ts` / `alertTemplates.ts` already point at the new paths).
- Existing `api/model_prices` consumption stays at `/model_prices` — rename the placeholder in `model_pricing.ts` accordingly.
- `docs/web/sitemap-diff.md`: flip P1 backend column from 🚧 → 🔌 for the affected rows.

## Capabilities

### New Capabilities

- `alert-templates`: Reusable alert notification templates with placeholder substitution.
- `regex-patterns`: Org-scoped VRL regex pattern shortcuts.
- `query-runtime-control`: In-process tracking + cancellation of running queries.

### Modified Capabilities

- `license`: Add `GET /api/v1/license` read endpoint (was spec'd as `/api/v1/system/license` — we keep the spec'd path and additionally expose the shorter `/license` alias that the frontend uses).
- `model-pricing`: Add the `/api/v1/model_prices` HTTP CRUD already mentioned in the spec but never landed.
- `copilot-telemetry`: Add `ai_toolsets` registry surface (OSS stub + enterprise Pg backing).

## Impact

- **Backend code**: 6 new route modules under `crates/api/src/http/routes/`; 4 new repos under `crates/infra/src/persistence/repositories/`; 2 new Postgres migrations (`alert_templates`, `regex_patterns`); QueryService gains an `Arc<RwLock<HashMap<QueryId, ActiveQuery>>>` registry.
- **State**: `AppState` gains `alert_templates`, `regex_patterns`, `ai_toolsets` repos.
- **Frontend**: 6 page files lose their `awaitingBackend` blocks; 3 new tiny clients land under `web/src/api/`.
- **i18n**: no namespace change — existing `backend_note` keys stay as fallback strings when a repo returns nothing.
- **a11y/lint/typecheck**: no new failures expected; the 16 Settings paths in `a11y-routes.spec.ts` already cover the affected pages.
- **Risk**: `query/running` registry adds runtime overhead per query (lock + map insert/remove). Mitigated by using a parking-lot RwLock and only registering on `execute_query` entry/exit; gRPC + flight server queries are out of scope.
- **OSS / enterprise split**: `ai_toolsets` repo follows the `actions` / `chat` pattern — OSS ships an empty `Arc<dyn AiToolsetRepository>` impl; `--features enterprise` wires the Pg-backed one. `model_prices` is OSS-safe (already compiled).
