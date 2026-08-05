## ADDED Requirements

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

The web app SHALL expose `/rum/source-maps` listing uploaded sourcemap files (release, file count, uploaded_at) and `/rum/upload-source-maps` for a drag-and-drop or paste-tar upload form. Both pages call `crates/api/src/http/routes/sourcemaps.rs`.

#### Scenario: Upload by release tag

- **WHEN** the user drops a `dist.tar.gz` on `/rum/upload-source-maps` and types release `v1.4.0`
- **THEN** the page issues `POST /api/v1/sourcemaps` with the release tag and multipart body
- **AND** on success navigates to `/rum/source-maps` with the new release highlighted
