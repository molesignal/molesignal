## ADDED Requirements

### Requirement: Virtualized Row Rendering

The log stream SHALL render rows using `@tanstack/react-virtual` with row height 24px in compact and 32px in comfortable mode; only rows visible in the viewport ± a 10-row overscan SHALL be mounted in the DOM. The component SHALL accept up to 1,000,000 rows in memory with constant scroll performance.

#### Scenario: DOM node count is bounded
- **WHEN** the dataset has 1,000,000 rows and the viewport shows 30 rows
- **THEN** at most ~50 row DOM nodes (visible + overscan + small buffer) are mounted at any time; this is verified by `document.querySelectorAll('[data-row]').length`

#### Scenario: Scroll performance under load
- **WHEN** the user scrolls continuously through a 1M-row stream on a modern laptop
- **THEN** average frame time stays under 16ms (measured via `PerformanceObserver` longtasks); no dropped frames exceeding 50ms

### Requirement: Field Coloring

Each row SHALL render fields in fixed order: `[timestamp][level][service][message]` with `timestamp` in muted, `level` colored by severity (`fatal/error = red`, `warn = yellow`, `info = default`, `debug = blue muted`), `service` in a hash-derived color from the same 9-color palette as `web-trace-view`, and `message` plain.

#### Scenario: Level color exact
- **WHEN** a row has `level = ERROR`
- **THEN** the level pill text uses CSS variable `--red` for color and `--red-bg` for background; on `level = WARN` it uses `--yellow` / `--yellow-bg`

#### Scenario: Service color stable
- **WHEN** the same `service = api` appears across many rows
- **THEN** the color is identical to whatever the trace view uses for `service = api` in the same session

### Requirement: Hover Preview

Hovering a row for >300ms SHALL show a right-floating mini-preview panel with the row's full JSON, syntax-highlighted, anchored to the row top. Moving the cursor off the row or pressing `Esc` closes the preview.

#### Scenario: Preview shows full JSON
- **WHEN** the user hovers a row whose JSON has 30 fields
- **THEN** the preview shows all 30 fields; values overflowing 200 chars are truncated with `…` and a `(full)` link expands

#### Scenario: Preview does not block scroll
- **WHEN** the preview is visible and the user starts scrolling
- **THEN** the preview closes immediately

### Requirement: Live Tail

A `Live` toggle in the header SHALL switch the stream to consume `GET /api/v1/query/stream?language=sql&statement=...&tail=true`; new rows append to the bottom and the scroll position SHALL auto-stick to bottom while the user is within 40px of the bottom, otherwise stay put with a `↓ new rows` badge.

#### Scenario: Sticky bottom while tailing
- **WHEN** live tail is on and the user is at the bottom
- **THEN** each incoming row keeps the view auto-scrolled to the new bottom; the badge does not appear

#### Scenario: New rows badge when scrolled up
- **WHEN** live tail is on and the user has scrolled up >40px
- **THEN** a badge `↓ <n> new rows` appears bottom-right; clicking it scrolls to bottom and clears the badge

### Requirement: Keyboard Operations

Inside the log stream the component SHALL bind: `j`/`k` to move the selection one row, `J`/`K` to move by 10 rows, `gg` to jump to the first row, `G` to jump to the last row, `Enter` to expand the selected row into a full-screen drawer with raw JSON + correlation actions, `⌘C` to copy the selected row's JSON to clipboard, and `/` to open a search input filtering rows by substring across all visible fields.

#### Scenario: j moves selection one row
- **WHEN** the user presses `j` with row 100 selected and 1M rows in the stream
- **THEN** row 101 becomes selected; if it was out of viewport the list scrolls just enough to bring it into view

#### Scenario: Enter pushes drawer
- **WHEN** the user presses `Enter` on a selected row
- **THEN** a `log_row` investigation frame is pushed containing the row's structured JSON and the correlation links (per `web-correlation`)

### Requirement: Inline Search

Pressing `/` SHALL focus an inline search bar; typing SHALL filter rows by a case-insensitive substring match across all field values; results SHALL be highlighted with the `accent` underline; `Esc` clears the filter.

#### Scenario: Filter narrows visible rows
- **WHEN** a 100k-row stream is loaded and the user types `panic`
- **THEN** only rows whose any string field contains `panic` are visible; matching text within those rows is underlined; non-matching rows are dropped from the virtualized window

#### Scenario: Esc clears
- **WHEN** the user has an active filter and presses `Esc`
- **THEN** the filter clears, the full row set is restored, and selection returns to the first visible row
