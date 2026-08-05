## Why

`web-feature-parity` shipped the P0 modules (RUM / Functions / Pipeline / IAM) and `web-feature-parity-settings` covered the 16 Settings sub-pages. The remaining ~14 secondary routes documented under `docs/web/sitemap-diff.md#P2` are still gaps: trace details, stream explore, service graph, dashboard import + new-panel, log inspector, PromQL builder, alerts history / insights, actions list, short-URL resolver, ingestion real pages. Every one of them already has a backend endpoint — only the web pages are missing.

## What Changes

### New routes (each wired to an existing backend route under `crates/api/src/http/routes/`)

- `/logs/inspector` — single search-job inspector (read `routes/search_jobs.rs`).
- `/metrics/promql-builder` — query builder UI on top of `routes/metrics.rs`.
- `/traces/:id` — trace detail view (read `routes/web/trace`).
- `/traces/session/:id` — session detail (same backend).
- `/streams/:id` — stream explore view (read `routes/web/streams`).
- `/service-graph` — service graph view (read `routes/web/topology`).
- `/dashboards/import` — JSON / YAML dashboard import drawer (existing dashboard endpoints).
- `/dashboards/:id/panels/new` — dedicated new-panel route (existing dashboard endpoints).
- `/alerts/history` — alert delivery history (read `routes/alerting.rs`).
- `/alerts/insights` — alert insight summaries (existing `/alerts/insights`).
- `/actions` — actions list (enterprise; OSS surface renders an awaiting-license empty state via `state.license.has_feature("actions")`).
- `/short/:id` — short-URL resolver page that calls `routes/short_url.rs` and `nav(replace=true)` to the long URL.
- `/ingest/:category/:source` — vendor-specific ingestion real pages replacing the current docs-only placeholders (4 categories × N vendors; logs / metrics / traces / RUM).

### Shell + nav updates

- Sidebar adds entry points where appropriate (Actions under DATA PLANE, Service Graph under OBSERVE, Alerts History under ALERTS, etc.).
- `a11y-routes.spec.ts` adds the new paths so axe `critical=0` is enforced.
- `docs/web/sitemap-diff.md` P2 table flipped to ✓ for landed rows.

### Non-Goals

- No new backend endpoints. Routes whose backend is still 🚧 in the diff (`alerts/import-semantic-groups`, `alerts/anomaly/*`) stay out of scope.
- No new feature surfaces; this change is strictly wiring missing pages to existing endpoints.
- No reshuffling of P0 / P1 routes that already shipped.

## Capabilities

### New Capabilities

- `web-misc-pages`: The page wirings landed by this change — one capability covering the list above so spec stays a single contract rather than fragmenting into 14 tiny capabilities. Each route is its own Requirement inside that spec.

### Modified Capabilities

- `web-shell`: Sidebar group additions for Actions / Service Graph / Alerts History / etc.; route table additions.

## Impact

- **Code**: ~14 new TSX route files under `web/src/routes/`; updates to `routes/index.tsx` and `shell/Sidebar.tsx`; new API clients only where an existing client doesn't already cover the endpoint (e.g. `api/serviceGraph.ts`, `api/alertInsights.ts`, `api/shortUrls.ts`).
- **i18n**: Reuse the existing `nav.json` + relevant feature namespaces; add a small `misc.json` only if needed for shared placeholder copy. No new namespaces for routes that fit inside an existing namespace (e.g. `alerts.history` lives under `alerts.json`).
- **a11y / lint / typecheck**: must remain green (`pnpm -C web typecheck / lint / test:run / a11y:contrast`); axe critical=0 on every new route via the existing a11y-routes spec.
- **Backend**: no changes.
- **Risk**: large surface but mechanically simple — every page is "client + useQuery + DataTable/details". Main risk is naming drift between the page and existing API clients; mitigated by reusing whichever clients are already present under `web/src/api/`.
