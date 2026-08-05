## Why

`web-feature-parity` landed RUM / Functions / Pipelines / IAM (4 modules / 21 routes) but deliberately punted Settings to a follow-up. Today `/settings` is a single page with 8 hard-coded mock sections. openobserve exposes 16 `/settings/*` sub-routes, several of which already have real molesignal backend endpoints (`cipher_keys.rs`, `connectors.rs`, `storage_providers.rs`, `domains.rs`, `clusters.rs`, `query.rs` running queries, `alerting.rs` channels/rules, `identity.rs` orgs). This change closes that gap so admins can reach every settings area via a stable URL.

## What Changes

### Routes — 16 sub-routes under `/settings/*`

- `/settings/general`, `/settings/organization`, `/settings/license` — profile + org metadata + plan (some backend-pending, render `EmptyState awaitingBackend`)
- `/settings/alert_destinations`, `/settings/alert_templates` — uses `routes/alerting.rs` (`/alerts/channels`, `/alerts/templates`)
- `/settings/pipeline_destinations` — uses `routes/connectors.rs` (`/connectors`)
- `/settings/cipher_keys` — uses `routes/cipher_keys.rs` (`/cipher_keys` + rotate)
- `/settings/regex_patterns` — backend-pending
- `/settings/ai_toolsets`, `/settings/model_pricing` — backend-pending (LLM ops)
- `/settings/query_management` — uses `routes/query.rs` (running queries) for now; needs `/query/running` aggregator endpoint to fully bloom
- `/settings/storage_settings` — uses `routes/storage_providers.rs` (`/clusters/storage_providers`)
- `/settings/nodes` — uses `routes/clusters.rs` (`/clusters`)
- `/settings/domain_management` — uses `routes/domains.rs` (`/domains`)
- `/settings/correlation` — uses `routes/web/correlation` (existing)
- `/settings/organization_management` — uses `routes/identity.rs` (`/orgs` + memberships)

### Shell + i18n + accessibility

- `routes/Settings.tsx` becomes a layout component with an internal SettingsSidebar listing 16 sections; deep links activate the matching section.
- Sidebar's ADMIN top-level "Settings" entry unchanged — internal sub-nav handles the 16 sections.
- New i18n namespace `settings-admin` (en + zh-CN) for the 16 sub-page strings.
- `a11y-routes.spec.ts` adds 16 new paths; existing 38 (11 base + 21 `web-feature-parity` + 6 sub-routes) remain.

### Admin skeleton reuse

Every sub-page reuses `web/src/admin/{PageHeader,DataTable,ConfirmDialog,EmptyState}.tsx` (landed in `web-feature-parity`) — no new skeleton components.

## Capabilities

### New Capabilities

- `web-settings-admin`: 16-section admin Settings hub. Each section has a stable URL, hits its corresponding backend (or renders `EmptyState awaitingBackend`), and respects org/role gates.

### Modified Capabilities

_None._ The Sidebar deep-link target switch (`/settings` → `/settings/general`) is an implementation detail of the routes added by `web-settings-admin`; the existing `web-shell` Sidebar requirement only enumerates entries, not target paths, so no delta is needed.

## Impact

- **Code**: 16 new files under `web/src/routes/settings/*.tsx`, expanded `routes/Settings.tsx` shell, new API clients in `web/src/api/` for endpoints not yet covered (`alertChannels`, `alertTemplates`, `connectors`, `cipherKeys`, `storageProviders`, `clusters`, `domains`, `runningQueries`). Existing `Settings.tsx` 8-section mock body retired.
- **Backend dependency**: ~9 of the 16 sub-pages bind to existing endpoints; the remaining 7 render an `awaitingBackend` empty state — no new backend work is gated.
- **i18n**: +1 namespace (`settings-admin`) ≈ 60 strings × 2 locales.
- **a11y**: a11y-routes spec gains 16 paths; critical=0 target unchanged.
- **Risk**: Settings is the single most heterogeneous module; mitigated by per-section page files, the shared admin skeleton, and an explicit "awaiting backend" affordance.
- **Follow-up**: `web-feature-parity-misc` (P2) covers main-page secondary routes + ingestion real pages + actions + short-url. No backend work is unblocked or blocked by this change.
