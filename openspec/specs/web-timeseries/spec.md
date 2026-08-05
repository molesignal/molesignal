# Web TimeSeries Plot

## Purpose

uPlot-based React wrapper for 24-series, 10M-point charts: LTTB downsampling, shift-drag brush vs pan, cursor sync across plots, linear/log/percentile y-axis modes, 9-color theme-aware palette.
## Requirements
### Requirement: uPlot React Wrapper

The web app SHALL expose a `TimeSeriesPlot` React component that wraps a single `uPlot` instance. The wrapper SHALL accept props `{ data: Series[], window: TimeRange, axes?: AxisSpec[], onRangeSelect?: (range) => void, onCursorMove?: (t) => void, theme: 'dark'|'light', height: number, downsampleHint?: number }` and SHALL avoid React reconciliation of uPlot's internal canvas.

#### Scenario: No re-render on cursor move
- **WHEN** the user moves the mouse over the chart
- **THEN** React does NOT re-render the `TimeSeriesPlot` (verified by render counter); only uPlot's canvas + the externally subscribed `CursorChannel` update

#### Scenario: Data update goes through setData
- **WHEN** the parent passes a new `data` array prop with the same `id`
- **THEN** the wrapper calls `uPlot.setData(newData)` rather than recreating the uPlot instance

### Requirement: Brush Range Selection

Holding `Shift` and drag-selecting horizontally on the chart SHALL emit `onRangeSelect({ from, to })`; releasing without `Shift` SHALL pan instead (move the global window in relative mode keeps it relative, otherwise mutates absolute window). **During a shift-drag, the active brush region SHALL render as a semi-transparent overlay with `background: var(--accent-bg)` and 1px `var(--accent)` left + right borders, and a one-line inline label `Brush: <hh:mm:ss> → <hh:mm:ss>` (UTC, monospace) SHALL follow the cursor. Drag-without-Shift (pan) SHALL NOT render the overlay or the label.**

#### Scenario: Shift-drag emits range

- **WHEN** the user holds `Shift`, presses at x corresponding to `09:30`, drags to x corresponding to `09:45`, and releases
- **THEN** `onRangeSelect` fires once with `{ from: 09:30, to: 09:45 }`; a visual selection rectangle remains for 500ms then fades

#### Scenario: Drag without Shift pans

- **WHEN** the user drags without `Shift` from x=09:30 to x=09:00
- **THEN** the global window shifts left by 30 minutes preserving width

#### Scenario: Brush region is clearly visible against series

- **WHEN** the user shift-drags from x=100 to x=400 over a plot
- **THEN** a translucent accent-colored rectangle covers x=100..x=400
- **AND** the inline label `Brush: 09:55:23 → 10:00:00` follows the cursor

#### Scenario: Pan does not paint a brush overlay

- **WHEN** the user drags without `Shift` held
- **THEN** the brush overlay rectangle is NOT shown and no inline label appears; the plot pans instead per `panWindow`

### Requirement: Cursor Synchronization Channel

The wrapper SHALL subscribe to the local `CursorChannel`; when the channel publishes a t-value originating elsewhere, the chart SHALL draw a crosshair at that t and render its tooltip at that t but SHALL NOT re-publish a synthetic event.

#### Scenario: External cursor draws crosshair
- **WHEN** a sibling chart publishes `cursor:09:42:31`
- **THEN** this chart draws a crosshair at that x within one animation frame; no `onCursorMove` is emitted from this chart

### Requirement: Axis Modes

The wrapper SHALL support three y-axis modes per axis: `linear`, `log`, `percentile`; `percentile` SHALL render as a non-uniform axis using the `[p50, p90, p95, p99, p99.9]` breakpoints supplied in the axis spec. Switching mode SHALL NOT recreate uPlot; only the scale config changes.

#### Scenario: Log mode hides non-positive points
- **WHEN** axis mode is `log` and a series has a value `<= 0`
- **THEN** that point is skipped in rendering; the series continues with a gap at that x; no console error is thrown

### Requirement: Downsample For Large Series

When a series has more points than the chart width in pixels times the supplied `downsampleHint` (default 2), the wrapper SHALL pre-aggregate using the `largest-triangle-three-buckets` (LTTB) algorithm before passing to uPlot; the output point count SHALL equal `chartWidth * downsampleHint`.

#### Scenario: 10M points downsampled
- **WHEN** the chart is 800px wide, `downsampleHint = 2`, and a series has 10,000,000 points
- **THEN** uPlot receives at most 1,600 points; the first paint completes within 80ms on a modern laptop (measured via `performance.mark`)

#### Scenario: Small series not downsampled
- **WHEN** a series has 200 points and chart width allows 1,600
- **THEN** all 200 points are forwarded unchanged

