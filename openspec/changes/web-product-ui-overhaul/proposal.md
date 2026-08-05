## Why

The current web frontend has broad route coverage, but it still behaves like a collection of parity pages rather than a coherent product for open-source operators,  platform teams, and SaaS buyers. As the backend and OpenObserve parity work mature, the frontend needs a product experience layer that makes value, activation, governance, and daily operations obvious without sacrificing the dense telemetry workflows.

## What Changes

- Establish a role- and edition-aware information architecture for OSS, , and SaaS users: Home, Observe, Data, Automation, RUM, Admin, and Growth/Billing surfaces have explicit entry points and page ownership.
- Replace inconsistent page bodies with a shared product UI system: page headers, KPI strips, data toolbars, empty states, license-gated states, drawers, forms, table actions, and detail layouts.
- Rework the authenticated shell into a dense product cockpit: persistent topbar, grouped sidebar, org/edition/cluster/time context, predictable breadcrumbs for deep pages, and accessible global actions.
- Add onboarding and activation flows that guide first-time users from empty workspace to first signal, including ingest setup, sample data, dashboard starter paths, and route-specific next steps.
- Add edition-aware UX for open source, , and SaaS: feature gates explain value and next action without showing raw 403s; SaaS surfaces trial, usage, billing, and upgrade hooks when available.
- Use `/Users/gagral/code/openobserve` as a parity/reference source for route topology and mature workflows, while keeping MoleSignal's React/Tailwind/shadcn-style implementation and product identity.
- Expand i18n requirements so all new product copy, gates, onboarding, and page templates ship in English and zh-CN.

## Capabilities

### New Capabilities

- `web-product-experience`: Product-level information architecture, route ownership, page taxonomy, role-aware navigation, and daily-workflow entry points.
- `web-design-system`: Shared frontend component patterns, page templates, density rules, responsive behavior, empty/error/loading states, and visual QA gates.
- `web-onboarding-activation`: First-run, empty-workspace, ingest setup, sample data, and guided activation experiences for OSS, , and SaaS.
- `web-edition-awareness`: Edition/plan/license/trial/upgrade UX that differentiates OSS, , and SaaS capabilities without blocking core OSS workflows.

### Modified Capabilities

- `web-shell`: Replace the older minimal shell contract with the current product-cockpit shell, including persistent grouped navigation, org switching, global search, breadcrumbs for deep pages, and responsive behavior.
- `web-command-palette`: Make global command search role/route/edition aware, with actions for activation, navigation, creation, support, and current context.
- `web-settings-admin`: Align settings pages with the new admin IA, page templates, edition gates, and SaaS/ account surfaces.
- `web-misc-pages`: Raise standalone parity pages from placeholders/route coverage to production page quality with consistent headers, query states, actions, and deep-link behavior.
- `web-i18n`: Extend translation requirements to product copy, onboarding, edition gates, empty states, and page template strings.

## Impact

- Affected code: `web/src/shell/*`, `web/src/routes/**/*`, `web/src/admin/*`, `web/src/i18n/**/*`, `web/src/palette/*`, `web/src/stores/*`, `web/src/shell/ui/*`, Playwright route/a11y/visual tests, and docs under `docs/web/`.
- API dependencies: mostly existing `/api/v1/*` endpoints; SaaS/billing/trial surfaces must degrade gracefully until backend contracts exist.
- Reference material: `/Users/gagral/code/openobserve/web/src/router/*`, OpenObserve route modules, and `docs/web/sitemap-diff.md`.
- Dependencies: no new frontend framework. Prefer existing React, Tailwind, lucide-react, Radix/shadcn-style primitives, TanStack Query, and existing visualization primitives.
- Risk: broad UI changes can regress dense operator workflows; implementation must be phased, page-template driven, and verified by typecheck, lint, unit tests, axe, and browser screenshots before large route migrations.
