# Web Command Palette

## Purpose

⌘K-driven command palette: cmdk-based modal merging a static action registry with remote /api/v1/web/search hits, fuzzy-ranked via fuzzysort, dispatching results into the investigation stack via Enter / ⌘Enter / ⌥Enter open modes.
## Requirements
### Requirement: Global Palette Activation

The web app SHALL provide a command palette opened by `⌘K` (macOS) / `Ctrl+K` (other), closed by `Esc`, that overlays the current view on a centered modal with a single input, a result list, and a footer hint strip showing context-sensitive key bindings.

#### Scenario: Open from any route
- **WHEN** the user presses `⌘K` from any authenticated route, including from inside a chart brush selection or a drawer
- **THEN** the palette opens, focus moves to its input, and the keyboard scope stack pushes `palette`

#### Scenario: Close restores prior scope
- **WHEN** the user presses `Esc` while the palette is open
- **THEN** the palette closes, the prior keyboard scope is restored, and focus returns to whatever element had focus before the palette opened

#### Scenario: Open while in input focus
- **WHEN** the user presses `⌘K` while focus is in a `<textarea>` or `<input>` (including the SQL editor)
- **THEN** the palette still opens and the original input is not modified

### Requirement: Action Registry

The palette SHALL combine results from a synchronous static action registry (commands the app declares at module load) with an asynchronous remote search; both result sets are merged, deduplicated by `kind+id`, and ranked by fuzzy match score using `fuzzysort` against the user query.

#### Scenario: Static action visible without query
- **WHEN** the palette opens with empty input
- **THEN** the result list shows at least the actions: `Switch organization…`, `Toggle theme`, `Open settings`, `Run SQL…`, `Run PromQL…`, `Pin current time`, `Copy investigation link` — in that order with no remote calls issued

#### Scenario: Query triggers remote search
- **WHEN** the user types at least 1 character
- **THEN** the palette debounces 80ms then issues `GET /api/v1/web/search?q=<query>&types=streams,dashboards,saved_views,alerts,incidents,services&limit=20`, and merges results with static actions, sorted by fuzzy score

#### Scenario: Ranking ties broken by recency
- **WHEN** two result items have the same fuzzy score
- **THEN** the more recently used item (per local `usedAt` log) ranks higher; if both unused, the one with `kind` in the priority order `[action, incident, saved_view, dashboard, stream, service, alert]` wins

### Requirement: Selection And Open Modes

The palette SHALL support three confirmation keys: `Enter` (open in current view, replacing any active drawer of the same kind), `⌘Enter` (open in a fresh investigation stack at root), and `⌥Enter` (push as a new layer on top of the current investigation stack).

#### Scenario: Plain Enter replaces same-kind frame
- **WHEN** the current investigation stack top is a `trace` frame and the user selects another trace via palette + `Enter`
- **THEN** the top frame is replaced (not stacked) with the new trace, preserving lower frames

#### Scenario: ⌘Enter resets stack
- **WHEN** the user confirms with `⌘Enter`
- **THEN** the investigation stack is cleared and a single root frame is created from the selection; the time anchor is preserved

#### Scenario: ⌥Enter pushes a layer
- **WHEN** the user confirms with `⌥Enter` on a `log` result while a `trace` frame is on top
- **THEN** a new `log` frame is pushed with `parent_frame_id` equal to the trace frame's id; cross-signal correlation (per `web-correlation`) prefills the log query with the trace's `trace_id` and time window

### Requirement: Result Item Contract

Every palette result item SHALL render with: a leading 16px icon (lucide-react), a primary label, an optional muted subtitle (e.g., stream name for a saved view), a right-aligned kind chip (`stream`, `service`, `incident`, `dashboard`, `saved_view`, `alert`, `action`), and a keyboard shortcut hint if the action has one. Selected row SHALL have a 2px `var(--accent)` left border and a `var(--accent-bg)` background at 12% alpha. **In `compact` density mode, subtitle SHALL truncate with ellipsis after a single line; in `comfortable` density, subtitle MAY wrap to a second line before truncating.**

#### Scenario: Required fields present

- **WHEN** any result is rendered
- **THEN** the row contains icon + label + kind chip at minimum; subtitle and shortcut hint are optional and may be absent

#### Scenario: Selected row is unambiguous

- **WHEN** the user navigates with `↓` / `↑` (or `j` / `k` when scope is `palette`)
- **THEN** exactly one row carries the selected state (2px accent border on its left edge + 12%-alpha accent-bg fill), and the list scrolls to keep that row in the viewport center band

#### Scenario: Compact mode does not clip kind chip

- **WHEN** the palette is open in `compact` density and a result item's subtitle is 80+ characters
- **THEN** the kind chip on the right is still fully visible
- **AND** the subtitle is ellipsised at the row's available width with `…`

#### Scenario: Selected row highlight

- **WHEN** a row is selected (via `data-selected="true"` cmdk attribute)
- **THEN** a 2px `var(--accent)` left border is visible
- **AND** the row background gets `var(--accent-bg)` at 12% alpha

### Requirement: Command Source Coverage

The static action registry SHALL include commands to (a) navigate to every top-level route in `web-shell`, (b) toggle the global time window across the presets `-5m -15m -1h -6h -24h -7d`, (c) toggle theme and density, (d) trigger SQL / PromQL editor in a new frame, (e) pin or unpin the current cursor as time anchor, (f) copy the current investigation URL to clipboard, (g) switch organizations, (h) open the `?` keyboard help overlay, (i) sign out.

#### Scenario: Time preset action
- **WHEN** the user opens the palette and selects `Time: last 1 hour`
- **THEN** the global time window store updates to `from: now - 1h, to: now` and all subscribed views re-query within 50ms

#### Scenario: Copy investigation link
- **WHEN** the user selects `Copy investigation link`
- **THEN** the palette writes the current location (including `?time`, `?anchor`, `?stack`) to clipboard and shows a toast `Link copied` for 1.5s

