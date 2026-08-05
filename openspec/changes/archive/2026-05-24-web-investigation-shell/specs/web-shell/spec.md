## ADDED Requirements

### Requirement: Minimal Application Chrome

The web app SHALL render a single application shell composed of: a 32px top status strip, a 52px left icon rail that is collapsed by default and hover-revealed within an 8px hot zone on the left edge, and a main content area that fills the remaining viewport with no breadcrumbs, page headers, or footers.

#### Scenario: Status strip shows fixed sections
- **WHEN** the app boots with an authenticated user
- **THEN** the status strip from left to right shows: org name, alive cluster count (`<n> nodes`), current global time window summary (`-1h` or `09:00 – 10:00 UTC`), the literal text `⌘K` as a hint, and the current user avatar+role; nothing else

#### Scenario: Left rail collapses by default
- **WHEN** the app first renders or a route changes
- **THEN** the 52px icon rail is not visible; pointer entering the leftmost 8px column expands it within 80ms; pointer leaving for >300ms collapses it

#### Scenario: No persistent navigation chrome
- **WHEN** any route is active
- **THEN** the DOM SHALL NOT render breadcrumb, page title bar, or page footer elements; the main area's top edge sits directly under the status strip

### Requirement: Theme And Density Tokens

The shell SHALL expose CSS custom properties for exactly nine semantic colors (`bg`, `surface`, `primary`, `accent`, `red`, `green`, `yellow`, `blue`, `purple`) per theme (`dark`, `light`), and two density modes (`compact`, `comfortable`) controlling row height and padding tokens, with `compact` as the default for authenticated routes.

#### Scenario: Theme tokens are limited
- **WHEN** a developer inspects `:root`
- **THEN** the only chrome color CSS variables defined are the nine semantic names (per theme) plus their `*-muted` and `*-bg` variants; no additional palette colors are exported

#### Scenario: Density default
- **WHEN** an authenticated user opens any route for the first time and has not set a density preference
- **THEN** the body element carries `data-density="compact"` and the row height token resolves to `28px`; switching to comfortable yields `36px`

### Requirement: Route Map

The shell SHALL register exactly the following routes: `/login`, `/investigate` (default authenticated landing), `/services`, `/services/:service`, `/dashboards`, `/dashboards/:id`, `/alerts/rules`, `/alerts/incidents`, `/alerts/incidents/:id`, `/saved-views`, `/settings/*`. All deep links SHALL re-hydrate the global time window, pinned anchor, and investigation stack from URL parameters before rendering data.

#### Scenario: Default authenticated landing
- **WHEN** a user logs in
- **THEN** the router redirects to `/investigate` with the global time window defaulted to the last 1 hour

#### Scenario: URL hydration before render
- **WHEN** a user opens a URL containing `?time=...&anchor=...&stack=...`
- **THEN** the app SHALL parse those parameters and update the time and stack stores synchronously before mounting the main view (no flash of empty state)

### Requirement: Auth Bootstrap

On app boot the shell SHALL read the JWT or `ms_` API token from secure storage, exchange it for `AuthContext { user_id, org_id, role }`, and either render the authenticated shell or redirect to `/login` preserving the original URL in `?next=`.

#### Scenario: Unauthenticated deep link
- **WHEN** an unauthenticated user navigates to `/investigate?time=-2h..now`
- **THEN** the app redirects to `/login?next=%2Finvestigate%3Ftime%3D-2h..now`; after successful login the user lands back on the original URL

#### Scenario: Token expiry mid-session
- **WHEN** any API call returns `401 token expired`
- **THEN** the shell clears stored tokens and redirects to `/login?next=<current>`; in-flight queries are cancelled

### Requirement: Web-Side Backend Contracts

The web app SHALL consume four web-aggregation endpoints provided by the backend: `GET /api/v1/web/search?q&types&limit`, `GET /api/v1/web/topology?from&to`, `GET /api/v1/web/trace/:trace_id`, and `GET /api/v1/web/correlation/:from_kind/:to_kind?ctx=<base64>`; plus a streaming variant `GET /api/v1/query/stream` (NDJSON over chunked or SSE). Each endpoint SHALL be scoped to the caller's org and return `200` on success or `4xx`/`5xx` with `{ "error": "<msg>" }`.

#### Scenario: Search aggregation respects types filter
- **WHEN** the palette issues `GET /api/v1/web/search?q=lat&types=streams,saved_views&limit=10`
- **THEN** the response is `{ "items": [{ "kind": "stream"|"saved_view", "id", "label", "subtitle"?, "icon"? }] }` containing only items of the requested types, total length <= 10

#### Scenario: Streaming query frames
- **WHEN** the client requests `GET /api/v1/query/stream?language=sql&statement=...` with `Accept: application/x-ndjson`
- **THEN** the server returns `200` with `Transfer-Encoding: chunked`, body is newline-delimited JSON where each line is a `RecordBatchFrame { rows: [...] }`, and a final `{"meta": {...}}` line closes the stream

#### Scenario: Cross-org request rejected
- **WHEN** a request to any `/api/v1/web/*` endpoint references an id owned by another org
- **THEN** the response is `404 Not Found` (no enumeration) and no payload is leaked
