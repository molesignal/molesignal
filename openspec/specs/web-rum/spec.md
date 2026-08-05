# Web RUM Capability

## Purpose

Provides the web UI for Real User Monitoring — session browsing, error tracking, performance dashboards (Core Web Vitals), and source map management.

## Requirements

### Requirement: RUM Sessions Browser

The web app SHALL expose `/rum/sessions` listing recent RUM sessions from `GET /api/v1/rum/sessions` with paging + time-window filter, and `/rum/sessions/view/:id` rendering the full session timeline (page loads, user interactions, JS errors, fetch calls) from `GET /api/v1/rum/sessions/:id`.

#### Scenario: Sessions list loads in last hour

- **WHEN** the user opens `/rum/sessions` with the default 1h window
- **THEN** the page issues `GET /api/v1/rum/sessions?from=<-1h>&to=<now>&limit=50`
- **AND** renders one row per session with `session_id`, user_id, country, browser, duration, error_count
- **AND** clicking a row navigates to `/rum/sessions/view/<id>`

#### Scenario: Session detail timeline

- **WHEN** the user opens `/rum/sessions/view/abc123`
- **THEN** the page issues `GET /api/v1/rum/sessions/abc123`
- **AND** renders a chronological timeline of every event in the session
- **AND** errors / slow fetches are color-coded

### Requirement: RUM Error Tracking

The web app SHALL expose `/rum/errors` listing JS errors with frequency + impacted-users counts, and `/rum/errors/view/:id` rendering a single error's stack trace + affected sessions.

#### Scenario: Errors aggregated by fingerprint

- **WHEN** the user opens `/rum/errors`
- **THEN** the page issues `GET /api/v1/rum/errors?from=<-24h>&to=<now>`
- **AND** rows show error message, fingerprint, occurrence count, unique users
- **AND** rows are sorted by occurrence count descending

#### Scenario: Error detail shows source-mapped stack

- **WHEN** the user opens `/rum/errors/view/<fp>`
- **THEN** the page renders the demangled stack trace (sourcemaps applied if uploaded)
- **AND** shows the last 10 affected sessions with deep-links to `/rum/sessions/view/:id`

### Requirement: RUM Performance Dashboards

The web app SHALL expose four performance sub-routes under `/rum/performance/{overview,web-vitals,errors,apis}`. Each renders aggregated Core Web Vitals or fetch performance metrics from the backend's RUM aggregate endpoints.

#### Scenario: Web Vitals quadrant view

- **WHEN** the user opens `/rum/performance/web-vitals`
- **THEN** the page renders LCP / FID / CLS / TTFB time-series for the active time window
- **AND** annotates each metric with "good / needs improvement / poor" thresholds

### Requirement: Source Maps Management

The web app SHALL expose `/rum/settings/source-maps` listing Web and mobile debug artifacts, plus `/rum/settings/source-maps/upload` for uploading one artifact with its application, service, release, kind, platform, architecture, and debug ID. Both pages call `src/api/http/routes/debug_artifacts.rs`.

#### Scenario: Upload by release tag

- **WHEN** the user selects `app.js.map` on `/rum/settings/source-maps/upload` and enters its application, service, and release identity
- **THEN** the page issues `POST /api/v1/debug-artifacts` with the multipart body
- **AND** on success navigates to `/rum/settings/source-maps` with the artifact listed

### Requirement: Mobile RUM datasource guides

The datasource catalogue SHALL provide separate guides for Web RUM, Flutter, Android native, and
iOS native. Each guide requires an explicit valid application ID before requesting its application-
bound `msrum_` token, displays the live receiver origin, and explains the relevant release debug
artifacts. uni-app SHALL NOT be listed.

#### Scenario: Flutter guide uses the current SDK contract

- **WHEN** a user selects Flutter RUM and confirms `application_id=checkout-mobile`
- **THEN** the page requests `GET /api/v1/auth/tokens/rum?application_id=checkout-mobile`
- **AND** the snippet uses package `molesignal_flutter`, `initRum`, `RumApp`, and
  `RumNavigationObserver`
- **AND** it explains `--obfuscate --split-debug-info` plus Flutter Symbols upload

#### Scenario: Native guides expose symbol requirements

- **WHEN** a user selects Android native or iOS native
- **THEN** the guide shows the `/api/v1/rum/*` write protocol and Bearer token
- **AND** Android covers R8 `mapping.txt`, NDK Build ID and unstripped `.so`
- **AND** iOS covers Mach-O UUID and the DWARF file inside the matching dSYM
