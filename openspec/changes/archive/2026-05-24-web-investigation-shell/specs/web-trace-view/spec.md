## ADDED Requirements

### Requirement: Trace Data Loading

The trace view SHALL fetch a trace via `GET /api/v1/web/trace/:trace_id` returning `Trace { trace_id, root_span_id, spans: Span[] }` where `Span = { span_id, parent_span_id?, service, operation, start_ns, end_ns, status, attributes, events[] }`. The view SHALL build the span tree in O(n) and reject the trace if more than one span has no parent (logging a `trace_invalid` browser metric).

#### Scenario: Single root enforced
- **WHEN** the response has zero or two parentless spans
- **THEN** the view renders an error state `Trace malformed: <n> root spans` and does not draw the canvas

#### Scenario: Up to 100k spans
- **WHEN** the trace contains 100,000 spans
- **THEN** the tree is built within 200ms (measured via `performance.mark`) and rendering proceeds

### Requirement: Render Mode Switch

The view SHALL offer two render modes: `flame` (stacked icicle, x = time, y = depth, width proportional to duration) and `waterfall` (rows ordered by start_ns, x = time, y = sequence). A header toggle and the `f`/`w` keys SHALL switch modes without reloading the trace.

#### Scenario: Mode toggle key
- **WHEN** the user presses `f` while looking at a waterfall
- **THEN** the view switches to flame mode with the same color and selection state preserved

#### Scenario: Mode persists in URL
- **WHEN** the user switches mode
- **THEN** the URL parameter `?trace_view=flame|waterfall` updates; deep links open in the requested mode

### Requirement: Canvas Rendering

The view SHALL render spans on a single `<canvas>` using `d3-scale` for x mapping and a manual color scale; spans narrower than 1px after culling SHALL be omitted from the draw call. The canvas SHALL be sized to device pixel ratio and re-rendered only on data change, viewport change, or scroll, never per mouse move.

#### Scenario: DPR scaling
- **WHEN** the device has `window.devicePixelRatio = 2`
- **THEN** the canvas's intrinsic `width` and `height` are 2× the CSS pixel size and `ctx.scale(2,2)` is applied once

#### Scenario: Tiny spans culled
- **WHEN** zoom level makes a span < 1px wide
- **THEN** that span is not drawn; a `+N more` aggregation marker may appear if its siblings are also culled, but rendering does not slow proportional to span count

### Requirement: Hover And Click

Hovering a span SHALL show an HTML tooltip (overlaid div, not canvas-drawn) with service, operation, duration, status; clicking a span SHALL push a new investigation frame `kind: 'trace_span'` containing the span details and structured attributes/events.

#### Scenario: Tooltip follows cursor
- **WHEN** the user moves the mouse over a span
- **THEN** the HTML tooltip updates its content and position within 1 animation frame; the canvas itself is not redrawn

#### Scenario: Click pushes drawer
- **WHEN** the user clicks a span
- **THEN** a new frame appears on the investigation stack with the span's data; the source trace remains in view behind

### Requirement: Coloring And Status Highlight

Spans SHALL be colored by `service` using a hash-to-palette mapping limited to the 9 semantic colors; spans with `status == ERROR` SHALL also receive a 1px `red` outer stroke; spans with `status == TIMED_OUT` SHALL be hatched with diagonal lines at 25% opacity.

#### Scenario: Same service consistent color
- **WHEN** ten spans share `service = checkout`
- **THEN** they all draw with the identical color across renders and across trace reloads in the same session

#### Scenario: Error highlight visible
- **WHEN** a span has `status = ERROR`
- **THEN** its rectangle has a 1px red stroke regardless of zoom level; on hover the tooltip prefix `ERROR · ` appears in red

### Requirement: In-Trace Search

Pressing `/` inside the trace view SHALL focus a search input; entering a query SHALL highlight matching spans by service or operation substring with a 2px `accent` outline and zoom the canvas to fit the first match; `n` / `N` cycle next / previous match.

#### Scenario: Highlight all matches
- **WHEN** the user types `payment`
- **THEN** all spans whose service or operation contains `payment` (case-insensitive) draw with the accent outline; their count is shown in the search bar `4 matches`

#### Scenario: Cycle matches
- **WHEN** the user presses `n` after a search
- **THEN** the canvas scrolls/zooms so the next match (in time order) is centered and selected
