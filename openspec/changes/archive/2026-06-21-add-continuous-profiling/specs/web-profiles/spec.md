## ADDED Requirements

### Requirement: Profiles Module Navigation

The web app SHALL register a Profiles module in the product IA (`web/src/product/ia.ts`) with an `investigate`-group route `/profiles` and a new `profiles` owner module, alongside detail (`/profiles/:id`) and compare (`/profiles/compare`) routes. Labels SHALL come from i18n keys, not hard-coded strings.

#### Scenario: Profiles route registered in IA

- **WHEN** the authenticated shell renders the investigate navigation group
- **THEN** a Profiles entry appears with an icon and an i18n label
- **AND** the IA registry declares its path, group, owner module `profiles`, icon, edition, role, and empty-state strategy

### Requirement: Flamegraph Browser

The Profiles route SHALL render a flamegraph browser that lets a user pick `service`, `profile_type`, and a time range, then renders the aggregated flamebearer with frame search/highlight, click-to-zoom drill-down, and a value-type switch (e.g. CPU time vs allocations).

#### Scenario: Render, search, and drill

- **WHEN** a user selects `service=api`, `type=cpu`, range `now-1h`
- **THEN** the flamegraph renders from `/api/v1/profiles/flamegraph`
- **AND** typing in frame search highlights matching frames
- **AND** clicking a frame zooms the view to that subtree

#### Scenario: Truncated result is surfaced

- **WHEN** the API responds with `truncated: true`
- **THEN** the UI shows a non-blocking notice that the merge was sampled

### Requirement: Differential View

The compare route SHALL let a user choose a baseline and a comparison window and render a diff flamegraph that visually distinguishes increases from decreases.

#### Scenario: Diff visualizes deltas

- **WHEN** a user sets baseline and comparison windows and runs the diff
- **THEN** frames that grew are colored as increases and frames that shrank as decreases

### Requirement: Trace and Service Correlation Entry

Profiles surfaces SHALL offer cross-signal navigation: a profile with `trace_id` links to the corresponding trace, and a trace/span detail surfaces an entry to view the flamegraph for that span window.

#### Scenario: Jump from profile to trace

- **WHEN** a viewed profile has a `trace_id`
- **THEN** the UI shows a link that opens the corresponding trace detail

### Requirement: Empty State and Onboarding

When an org has no profiles in the selected window, the Profiles route SHALL render an activation empty state with copyable setup snippets for OTLP Profiles, Pyroscope ingest, and pprof upload, instead of a blank table.

#### Scenario: Empty org gets ingest guidance

- **WHEN** an org has ingested no profiles
- **THEN** the page shows setup snippets for at least the three supported ingress paths
- **AND** each snippet is copyable

### Requirement: Localized Copy

All Profiles UI copy (labels, empty states, gates, errors) SHALL ship in both `en-us` and `zh-cn` with no missing keys.

#### Scenario: No missing keys

- **WHEN** the Profiles module renders in `en-us` and in `zh-cn`
- **THEN** no raw i18n key is displayed in either locale
