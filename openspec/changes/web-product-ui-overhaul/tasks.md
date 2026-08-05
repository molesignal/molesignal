## 1. Product IA Foundation

- [x] 1.1 Add a typed product IA registry for route group, label key, icon, edition, role, owner module, and empty-state strategy.
- [x] 1.2 Refactor Sidebar and Topbar navigation to read from the IA registry instead of duplicating route metadata.
- [x] 1.3 Add breadcrumb/back metadata for deep routes such as dashboard edit, trace detail, stream explore, inspector, import, and builder pages.
- [x] 1.4 Update docs/web navigation documentation to describe the new Home / Observe / Data / Automate / Admin grouping.

## 2. Design System Primitives

- [x] 2.1 Create shared page templates: OverviewPage, ListPage, DetailPage, BuilderPage, SettingsPage, and GatePage.
- [x] 2.2 Extend admin primitives for standard PageHeader, toolbar, KPI strip, action bar, filter area, and metadata strip.
- [x] 2.3 Create shared state components for loading, empty, error, backend-pending, permission-denied, and license-gated states.
- [x] 2.4 Add template-level i18n keys in new product/design-system namespaces for en-us and zh-cn.
- [x] 2.5 Add Story/demo or lightweight route fixtures for page templates so visual QA can cover them without live backend data.

## 3. Shell Upgrade

- [x] 3.1 Align AppShell, Topbar, Sidebar, and StatusBar with the product-cockpit shell spec.
- [x] 3.2 Add responsive shell behavior for 375px, 768px, 1024px, and 1440px viewports.
- [x] 3.3 Add accessible mobile navigation trigger and verify keyboard access to org switcher, command palette, settings, and user menu.
- [x] 3.4 Replace outdated StatusStrip references in UI copy, specs-facing comments, and tests with topbar/shell terminology where applicable.
- [x] 3.5 Add Playwright shell screenshots for desktop and mobile shell states.

## 4. Edition Awareness

- [x] 4.1 Add a normalized edition metadata store for deployment mode, license features, SaaS trial state, role permissions, and backend-pending features.
- [x] 4.2 Implement FeatureGate, FeatureBadge, and GatePage variants for -required, SaaS-only, trial-available, permission-denied, and backend-pending states.
- [x] 4.3 Wire AI toolsets, domains, federated nodes, and SaaS/account routes through shared gates instead of raw 403 handling.
- [x] 4.4 Add localized gate copy and next actions for OSS, , and SaaS modes.
- [x] 4.5 Add unit tests for edition metadata fallback and gate selection.

## 5. Onboarding And Activation

- [x] 5.1 Build first-run activation state derivation from streams, dashboards, alerts, pipelines, and sample-data availability.
- [x] 5.2 Refactor Home into an OverviewPage with activation checklist, operational KPIs, recent work, and next best actions.
- [x] 5.3 Upgrade Ingest source pages with endpoint display, copyable snippets, validation guidance, and test-event state.
- [x] 5.4 Add sample-data action UI that degrades to backend-pending when no endpoint exists.
- [x] 5.5 Add Playwright coverage for empty OSS org activation and active org Home summary.

## 6. Command Palette

- [x] 6.1 Extend the static action registry to include IA routes, creation flows, onboarding actions, support/account actions, and gated actions.
- [x] 6.2 Add route-context ranking so detail pages surface resource-specific actions.
- [x] 6.3 Add edition-aware command filtering and gated command behavior.
- [x] 6.4 Add tests for empty-org action ordering, dashboard detail actions, and self-hosted license command search.

## 7. Route Family Migration

- [x] 7.1 Migrate Dashboards, Alerts, Streams, Functions, Pipelines, and Reports list routes to ListPage.
- [x] 7.2 Migrate Trace detail, Session detail, Stream explore, Dashboard detail/edit, and Logs inspector to DetailPage or BuilderPage.
- [x] 7.3 Migrate Settings and IAM sub-pages to SettingsPage/ListPage templates with explicit ownership and cross-links.
- [x] 7.4 Upgrade RUM pages with consistent tabs, empty states, source-map actions, and performance dashboard framing.
- [x] 7.5 Remove route-local one-off empty/error/loading states once equivalent shared components are in use.

## 8. i18n And Copy Audit

- [x] 8.1 Add i18n namespaces for product IA, onboarding, design-system states, and edition gates.
- [x] 8.2 Migrate new and touched page copy to i18n keys in en-us and zh-cn.
- [x] 8.3 Add a targeted missing-key test for every new namespace.
- [x] 8.4 Add or extend a static copy audit for migrated route directories.

## 9. QA And Performance Gates

- [x] 9.1 Run and fix `pnpm -C web run typecheck`.
- [x] 9.2 Run and fix `pnpm -C web lint`.
- [x] 9.3 Run and fix relevant unit tests for stores, i18n, command palette, and migrated components.
- [x] 9.4 Run Playwright a11y coverage for migrated routes and ensure zero critical axe violations.
- [x] 9.5 Capture browser screenshots for Home, Ingest, one Observe route, one Data route, one Admin route, one gated route, and mobile shell.
- [x] 9.6 Verify no migrated page shows raw i18n keys, raw 403 payloads, blank backend-pending tables, or clipped button text.
