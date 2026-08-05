## Context

The web frontend is a React/Tailwind application with broad route parity against OpenObserve: logs, metrics, traces, dashboards, streams, pipelines, functions, RUM, IAM, settings, and misc detail routes are present. The current implementation has strong low-level pieces (strict TypeScript, i18n, theme tokens, shadcn/Radix primitives, query states, visualization primitives), but the product layer is inconsistent: some pages are dense operational tools, some are placeholders, some use custom local layouts, and the shell/specs still carry older minimal-chrome assumptions that no longer match the visible topbar/sidebar product.

The target audience is split across three buying/usage modes:
- Open-source community users need fast local activation, transparent feature boundaries, and self-serve documentation in the UI.
-  users need governance, admin, license, security, audit, SSO, storage, cluster, and support surfaces that feel reliable and controlled.
- SaaS users need account, trial, usage, billing/upgrade, org switching, and support/feedback affordances without interrupting core telemetry workflows.

OpenObserve is used as a product topology reference, not as a UI framework to copy. MoleSignal remains React/Tailwind/lucide/Radix-based and should preserve its quiet, data-dense visual identity.

## Goals / Non-Goals

**Goals:**
- Define a product IA that makes daily work and setup flows obvious across OSS, , and SaaS.
- Standardize page templates and component patterns so every route has consistent headers, actions, loading/error/empty states, and accessible interactions.
- Make edition and license boundaries understandable without exposing raw API errors or dead-end pages.
- Upgrade the shell from route chrome into a product cockpit: persistent context, global search, grouped nav, breadcrumbs, org switching, settings, and status.
- Create a phased migration path that can be implemented incrementally without destabilizing query/viz-heavy routes.

**Non-Goals:**
- Replacing React, Tailwind, Radix/shadcn-style primitives, TanStack Query, or the existing visualization stack.
- Creating a marketing website or public landing page outside the authenticated app.
- Building backend billing, trial, license, or usage APIs in this change.
- Copying OpenObserve Vue/Quasar implementation details directly.
- Redesigning low-level chart rendering algorithms unless a page template requires better framing or state handling.

## Decisions

### D1: Product IA Is Workflow-First, Not Route-Parity-First

The primary navigation groups will be:
- Home: operational overview, activation, health, and recently used work.
- Observe: Logs, Metrics, Traces, Service graph, RUM, Dashboards, Alerts.
- Data: Ingest, Streams, Pipelines, Functions, Enrichment tables, Reports.
- Automate: scheduled reports/backfills, alert workflows, and automation entries.
- Admin: IAM, Settings, License, Organization, Storage, Domains, Query management.

Rationale: OpenObserve parity ensures coverage, but MoleSignal needs first-class product hierarchy for scanability. This grouping maps to how operators work: observe signals, manage data flows, automate responses, administer the platform.

Alternative considered: keep existing parity grouping. Rejected because it leaves activation, /SaaS account surfaces, and page ownership scattered.

### D2: Page Templates Replace One-Off Layouts

Introduce a small set of route-level templates:
- `OverviewPage`: KPI strip, health summary, activation tasks, recent work.
- `ListPage`: title/actions, filter toolbar, DataTable, bulk actions, empty/error states.
- `DetailPage`: back/breadcrumb, summary, tabs, related resources, side facts.
- `BuilderPage`: editor canvas, inspector/sidebar, validation footer.
- `SettingsPage`: settings section nav, form/list body, policy/permission hints.
- `GatePage`: license/edition/backend-pending state with next action.

Rationale: Consistent structure improves professional polish faster than per-page redesigns. It also makes a11y and responsive checks reusable.

Alternative considered: redesign each page independently. Rejected because it creates visual drift and makes QA unbounded.

### D3: Keep Dense Operator UI, Avoid Marketing Composition Inside App

Authenticated routes must prioritize scanning, comparison, filtering, and repeated action. Use compact spacing, stable grid/table dimensions, restrained cards, clear section bands, and token-based color. Avoid oversized hero sections, decorative cards, gradient backgrounds, and explanatory copy blocks that displace operational controls.

Rationale: The product is an telemetry tool for repeated professional use. The app can sell value through useful gates and activation, not marketing-style layout.

Alternative considered: SaaS-style dashboard with large hero and decorative onboarding panels. Rejected because it harms daily operator workflows.

### D4: Edition Awareness Is a UX Contract

Create a shared `EditionGate` and `FeatureBadge` model that can render:
- OSS included
-  required
- SaaS-only
- Trial available
- Backend pending
- Permission denied

Every gated page must show what the feature does, why it is gated, and the next action: configure license, contact sales, start trial, read docs, or continue with OSS alternative.

Rationale: OSS users should not feel punished by hidden  features; /SaaS users should see a clear path to value. Raw 403s are product failures.

Alternative considered: hide every unavailable route. Rejected because open-source deployments benefit from discoverability and transparent roadmap/value.

### D5: OpenObserve Is the Workflow Reference

Use OpenObserve for:
- Route topology and module completeness.
- Mature workflows such as ingestion, RUM, alerts, pipelines, dashboards, settings, and IAM.
- Copying intent and required page states, not implementation.

Do not copy:
- Vue/Quasar component structure.
- Exact visual styling.
- Any route that does not match MoleSignal's backend/product strategy.

Rationale: Reference reduces missed workflows without importing another design system.

### D6: i18n and Accessibility Are Built Into the Templates

All template strings, gate copy, onboarding tasks, nav labels, and empty states must live in i18n namespaces. Page templates must provide heading hierarchy, focus order, keyboard-accessible actions, and axe route coverage by default.

Rationale: The product targets open-source and  buyers; accessibility and localization are part of quality, not cleanup.

### D7: Migration Is Phased by Risk

Phases:
1. Foundation: shell contract, IA registry, page template primitives, edition gate primitives, i18n keys, visual snapshots.
2. Home + onboarding: activation dashboard, ingest paths, sample data, first dashboard/log query path.
3. Observe/Data route migration: logs, metrics, traces, streams, dashboards, alerts, pipelines, functions.
4. Admin//SaaS route migration: IAM, settings, license, usage/billing placeholders, support/contact flows.
5. Polish and QA: responsive pass, a11y route matrix, copy audit, route screenshots, performance sanity checks.

Rationale: This keeps high-risk query/viz pages stable while the design system proves itself on lower-risk pages.

## Risks / Trade-offs

- [Risk] Large visual refactors can break operator muscle memory. -> Mitigation: preserve route URLs, common shortcuts, command palette entries, and dense table/query workflows; migrate page templates incrementally.
- [Risk] Edition gates can feel like upsell spam to OSS users. -> Mitigation: gates must provide OSS alternatives or docs and stay outside critical open-source paths.
- [Risk] More product copy increases i18n burden. -> Mitigation: namespace new copy by feature and add missing-key tests for every new namespace.
- [Risk] Responsive requirements conflict with dense dashboards. -> Mitigation: desktop remains primary for complex telemetry, but navigation, settings, onboarding, lists, and basic query views must be usable at tablet/mobile widths.
- [Risk] Backend gaps can leave polished UI with no data. -> Mitigation: every backend-pending page uses `GatePage` / `EmptyState awaitingBackend` with explicit endpoint expectations.
- [Risk] OpenObserve reference can bias toward cloning. -> Mitigation: every referenced workflow must be re-expressed through MoleSignal templates and verified against MoleSignal backend contracts.

## Migration Plan

1. Add product IA registry and shell metadata without changing routes.
2. Add shared templates and edition/onboarding primitives behind existing pages.
3. Migrate Home and Ingest first because they shape first-run activation.
4. Migrate route families one by one, keeping per-family Playwright screenshots.
5. Update specs/docs after each family lands.
6. Remove obsolete minimal-chrome assumptions once the new shell spec is archived.

Rollback strategy: template migrations are route-local. If a migrated route regresses, revert that route to its previous body while keeping shared primitives in place.

## Open Questions

- What exact SaaS billing/usage endpoints will exist, and which deployment modes should hide billing entirely?
- Should MoleSignal expose a "sample data" generator through an API, local fixture import, or client-only demo path?
- Which  features should remain visible-but-gated in OSS versus hidden until license metadata is loaded?
- Should the current `min-w-[1280px]` shell constraint remain for every authenticated route, or only for dense query/viz workspaces?
