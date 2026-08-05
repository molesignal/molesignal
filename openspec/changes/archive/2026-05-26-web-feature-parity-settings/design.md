## Context

`web/src/routes/Settings.tsx` today is a single 700-line component with eight sections rendered as tabs inside one route (`/settings`, `/settings/*` mounted on the same component). Most sections are mocked. openobserve organizes the same surface as 16 first-class sub-routes under `/settings/*`, each backed by its own endpoint group.

`web-feature-parity` already landed the admin skeleton (`web/src/admin/{PageHeader,DataTable,ConfirmDialog,EmptyState}.tsx`) and the pattern of degrading to an "awaiting backend" state when an endpoint isn't yet implemented. This change reuses both — no new chrome work.

Backend reality (from `crates/api/src/http/routes/`):

| Sub-page | Endpoint(s) | Status |
| --- | --- | --- |
| general | profile + `/users/:id` | partial (no profile endpoint yet) |
| organization | `/orgs/:id` | exists |
| alert_destinations | `/alerts/channels` | exists |
| alert_templates | `/alerts/templates` | exists (under alerting.rs) |
| pipeline_destinations | `/connectors` | exists |
| cipher_keys | `/cipher_keys` + rotate | exists |
| regex_patterns | — | pending |
| ai_toolsets | — | pending |
| model_pricing | — | pending |
| query_management | `/query/running` aggregator | partial — `/query/inspect` exists, list endpoint pending |
| storage_settings | `/clusters/storage_providers` | exists |
| nodes | `/clusters` | exists |
| domain_management | `/domains` + renew | exists |
| correlation | `/web/correlation/*` | exists |
| organization_management | `/orgs`, `/orgs/:id/members` | exists |
| license | `/license` | pending |

## Goals / Non-Goals

**Goals:**
- 16 first-class sub-routes under `/settings/*`, each a routed page (no more sibling tabs in a monolith).
- One internal SettingsSidebar listing the 16 sections; deep links land on the right section with the right URL.
- Pages that bind to real endpoints show real data via the existing `QueryState` / `queryStateFor` pattern; pages backed by a missing endpoint render `EmptyState awaitingBackend` so the surface still exists.
- All strings go through the new `settings-admin` i18n namespace; existing keys in `common.json` / `nav.json` reused.
- a11y-routes auto-spec gains the 16 paths; critical=0 maintained.

**Non-Goals:**
- Not implementing the missing backend endpoints (regex_patterns, ai_toolsets, model_pricing, query running aggregator, license). Those are tracked as backend follow-ups.
- Not redesigning the Settings layout pixel-by-pixel against openobserve. Visual language remains molesignal's.
- Not introducing CodeMirror or a richer editor for regex_patterns / ai_toolsets — those pages render simple forms once their endpoints land.
- Not adding new admin skeleton primitives. Anything page-level that doesn't fit `PageHeader / DataTable / ConfirmDialog / EmptyState` either reuses `Form*` from `shell/FormDrawer.tsx` or is implemented inline.

## Decisions

### D1: One file per sub-page, shared SettingsLayout

```
web/src/routes/settings/
├── SettingsLayout.tsx       # internal sidebar + <Outlet/>
├── General.tsx
├── Organization.tsx
├── License.tsx
├── AlertDestinations.tsx
├── AlertTemplates.tsx
├── PipelineDestinations.tsx
├── CipherKeys.tsx
├── RegexPatterns.tsx
├── AiToolsets.tsx
├── ModelPricing.tsx
├── QueryManagement.tsx
├── StorageSettings.tsx
├── Nodes.tsx
├── DomainManagement.tsx
├── Correlation.tsx
├── OrganizationManagement.tsx
└── index.ts
```

`SettingsLayout` mounts under `/settings`; `<Outlet />` renders the selected sub-page; clicking a sidebar item navigates to the corresponding sub-route. Alternative considered: a giant tab-controlled monolith (current state). Rejected — URLs are not deep-linkable and code is hard to split.

### D2: Existing `Settings.tsx` shell retired in place

Rather than parallel-tracking the existing monolith, this change deletes its mock sections in favor of the routed sub-pages. The Sidebar's ADMIN > Settings entry switches from `/settings` to `/settings/general` as the default landing path. The legacy `/settings/*` wildcard route in `routes/index.tsx` is replaced by explicit per-section routes.

### D3: Per-section API clients live under `web/src/api/`

Settings touches 9 distinct endpoint groups. Rather than one mega `settings.ts` client, each gets its own file mirroring backend routes:

- `api/alertChannels.ts` (✓ may extend existing alerts.ts)
- `api/alertTemplates.ts`
- `api/connectors.ts`
- `api/cipherKeys.ts`
- `api/storageProviders.ts`
- `api/clusters.ts` (nodes)
- `api/domains.ts`
- `api/runningQueries.ts`

Already present and reusable: `orgs.ts` (org_management), `users.ts` (general profile section), `web.ts` (correlation).

### D4: "Awaiting backend" affordance per page

Pages whose endpoint is missing follow the established pattern: render `<EmptyState awaitingBackend title="…" description="…" />` in place of the data list. This:

1. Keeps the sub-route reachable so the IA matches openobserve.
2. Makes it obvious to engineers + admins that the gap is intentional and tracked.
3. Lets the page flip to real data with a single client wiring once the endpoint lands.

### D5: i18n namespace `settings-admin`

One namespace for all 16 pages. Per-section subtree under each key (`general.*`, `organization.*`, etc.). The Sidebar's existing `nav.json` `settings` key is unchanged; the new namespace owns only the section bodies.

### D6: Sub-nav ordering reflects information frequency

Sidebar order (top → bottom, grouped):

```
ACCOUNT
  general
  organization
  license

DATA PLANE
  storage_settings
  pipeline_destinations
  nodes
  correlation

ALERTS
  alert_destinations
  alert_templates

SECURITY
  cipher_keys
  regex_patterns
  domain_management
  organization_management

ML / OPS
  ai_toolsets
  model_pricing
  query_management
```

Alternative: alphabetical or backend-source ordering. Rejected — semantic grouping helps admins navigate.

## Risks / Trade-offs

**[R1] Deleting existing `Settings.tsx` body removes 8 mocked sections that have visible content today.**
→ Mitigation: the routed replacements either bind to real endpoints (most) or surface `awaitingBackend`. Net effect: more honesty, less mock data; admins lose no real functionality.

**[R2] The internal SettingsSidebar duplicates concept of the top-level Sidebar.**
→ Mitigation: distinct visual treatment (smaller, no group headers, no icons), and the `/iam` module already established this two-level pattern.

**[R3] 16 sub-routes inflate route-table size + bundle.**
→ Mitigation: each sub-page is ~80-150 LoC; current build code-splits per-route via lazy where needed. Even un-split, 16 pages × ~1.5 KB gz ≈ +24 KB.

**[R4] Several pages will land with `awaitingBackend` empty states — risk of "looks unfinished".**
→ Mitigation: the badge "Awaiting backend" is explicit; description states the missing endpoint name; once backend lands one PR wires the client.

**[R5] `query_management` requires a `/query/running` list endpoint that doesn't exist today; only `/query/inspect` does.**
→ Mitigation: ship the page with `awaitingBackend`; add a one-line note + tracking-issue link. Wiring the inspect endpoint by id is trivial later.
