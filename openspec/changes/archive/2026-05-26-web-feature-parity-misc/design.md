## Context

P0 (RUM / Functions / Pipeline / IAM) and P1 (Settings 16 sub-pages) shipped. The leftover P2 list in `docs/web/sitemap-diff.md` is ~14 routes whose backend endpoints already exist under `crates/api/src/http/routes/`. The work is mechanical: write a TSX page, wire `useQuery` to the right client, hook the page into `routes/index.tsx` and Sidebar, add to `a11y-routes.spec.ts`. The risk is sprawl rather than complexity — 14 pages × loading / empty / error / a11y / i18n is enough surface to do sloppily.

## Goals / Non-Goals

**Goals:**
- 14 new web routes landed, each rendering real data from an existing endpoint.
- Every new route: `axe critical=0`, has skeleton / empty / error states, listed in `a11y-routes.spec.ts`.
- `docs/web/sitemap-diff.md` P2 table updated row-by-row.
- Reuse existing API clients; only add a new client when no existing one covers the endpoint.
- Lint / typecheck / unit / a11y:contrast all stay green.

**Non-Goals:**
- No backend work — routes whose backend is still 🚧 (`alerts/import-semantic-groups`, `alerts/anomaly/*`) stay out of scope.
- No new feature surfaces beyond the existing endpoint's contract — e.g. service-graph page renders what `routes/web/topology` returns; if topology returns no edge data the page shows the empty state.
- No new design system primitives. Use existing `PageHeader / DataTable / EmptyState / KvRow / QueryState / FormDrawer`.
- No new visualizations beyond what the existing primitives already support (timeseries / topology / trace).
- No i18n namespace explosion — reuse existing namespaces (`logs`, `metrics`, `traces`, `streams`, `dashboards`, `alerts`, `actions`, `ingestion`) and only introduce a `misc.json` if a page genuinely doesn't fit anywhere.

## Decisions

### D1: One umbrella capability `web-misc-pages`

Each new route is a `### Requirement: <Route Name>` inside one `web-misc-pages` spec. Fragmenting into 14 micro-capabilities would multiply boilerplate for no value. The spec stays a flat list keyed by route; each requirement carries a single scenario describing the happy path + one for the empty/error state when meaningful.

Alternative considered: per-area capabilities (`web-logs-inspector`, `web-trace-detail`, etc.). Rejected — these aren't independent capabilities, they're page wirings under the existing capabilities (`web-log-stream`, `web-trace-view`, `web-topology`, etc.). One catch-all keeps the spec set tidy.

### D2: Backend route → existing API client mapping

For each new page, prefer an existing `web/src/api/<feature>.ts` client. Add a new client only when none covers the endpoint:

| Page | Endpoint | API client |
|---|---|---|
| `/logs/inspector` | `/search_jobs/:id` | reuse `api/searchJobs.ts` (already exists? otherwise new) |
| `/metrics/promql-builder` | `/metrics/query` | reuse `api/metrics.ts` / `api/query.ts` |
| `/traces/:id` | `/web/trace` | reuse `api/traces.ts` |
| `/traces/session/:id` | `/web/trace` (filter by session) | reuse `api/traces.ts` |
| `/streams/:id` | `/web/streams/:id` | reuse `api/streams.ts` |
| `/service-graph` | `/web/topology` | new `api/serviceGraph.ts` (no existing client) |
| `/dashboards/import` | `POST /dashboards` | reuse `api/dashboards.ts` |
| `/dashboards/:id/panels/new` | `PATCH /dashboards/:id` | reuse `api/dashboards.ts` |
| `/alerts/history` | `/alerts/history` | new `api/alertHistory.ts` if absent |
| `/alerts/insights` | `/alerts/insights` | new `api/alertInsights.ts` if absent |
| `/actions` | `/actions` (enterprise) | new `api/actions.ts` if absent |
| `/short/:id` | `/short/:id` | new `api/shortUrls.ts` if absent (client-side `nav(replace=true)`) |
| `/ingest/:category/:source` | `/ingestion/*` | reuse `api/ingestion.ts` |

The implementing PR audits which clients exist before adding new ones.

### D3: `/short/:id` is a redirect-only page

Lands a tiny `ShortUrlRedirect.tsx` that:
1. Resolves the short code via `/short_url/:code` (the backend returns the long URL).
2. Calls `nav(longUrl, { replace: true })` on success.
3. Renders a one-line "Resolving…" + error state on failure.

No layout chrome (PageHeader / Sidebar) — the route is presentation-less.

Alternative considered: server-side redirect via 302. Rejected — the existing pattern is client-side; the backend already returns JSON.

### D4: `/actions` OSS surface

The endpoint is enterprise-gated. In OSS the page renders the same shell but reads `state.license.has_feature("actions")` (already exposed via `/license` from `backend-settings-endpoints`). When false, render an `EmptyState` with "Actions require an enterprise license" — no awaitingBackend flag because the backend is reachable, just disabled.

### D5: Sidebar additions

- DATA PLANE group: `Actions` (new entry, between Pipelines and Connectors)
- OBSERVE group: `Service graph` (new entry, between Traces and RUM)
- ALERTS sub-area: `History`, `Insights` (sub-nav on `/alerts` shell, not top-level Sidebar)
- Logs / Metrics / Traces / Streams secondary routes are reachable via in-page entries (e.g., trace row → trace detail), not Sidebar.

### D6: `/ingest/:category/:source` is the canonical real-page path

Existing routes `/ingest/:category/:source` already render docs-only placeholders. This change replaces the placeholders with real per-vendor pages that show a snippet + auto-detected endpoint URL + a "Test event" button hitting `/ingestion/_health`. The set of vendors mirrors `web/src/data/ingestionVendors.ts` (or equivalent existing fixture). No URL changes.

## Risks / Trade-offs

**[R1] Sprawl — 14 pages is a lot of small files**
→ Mitigation: one umbrella capability (D1) keeps the spec compact; review checklist enforces loading/empty/error/a11y/i18n per page.

**[R2] `/actions` OSS check needs license snapshot client**
→ Mitigation: already shipped by `backend-settings-endpoints` (`/license` endpoint + `web/src/api/license.ts`). Page reuses it via `useQuery(['license-snapshot'])`.

**[R3] Trace / topology pages depend on backend response shapes that aren't strongly typed in web yet**
→ Mitigation: introduce minimal TypeScript types in each client matching the actual JSON; do not over-spec — match what the handler returns.

**[R4] Sidebar growth — too many entries cause cognitive load**
→ Mitigation: only top-level entries added are Actions + Service graph; alerts history / insights live under an in-page sub-nav.

**[R5] Playwright suite grows with each new route added to `a11y-routes.spec.ts`**
→ Mitigation: spec uses a flat array; each new route is one line. CI duration impact stays modest.

## Migration Plan

1. Audit existing `web/src/api/*` to know which clients can be reused.
2. Land new API clients (only the ones missing).
3. Land routes in batches by area (logs/metrics → traces/streams → dashboards/alerts → actions/short-url → ingestion) — each batch its own commit so revert is straightforward.
4. Update `routes/index.tsx`, `shell/Sidebar.tsx`, `a11y-routes.spec.ts`, `docs/web/sitemap-diff.md` together in a final commit.
5. Run `pnpm -C web typecheck / lint / test:run / a11y:contrast`. Run `openspec validate web-feature-parity-misc --type change --strict`.

Rollback: each batch is its own commit; revert one to drop a single area.
