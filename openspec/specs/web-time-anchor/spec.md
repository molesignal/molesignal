# Web Time Anchor

## Purpose

Global time window store (relative + absolute) + pinned anchor with halo expansion per signal kind (trace_span ±30s / log_row ±5s / metric_sample ±60s) + cross-frame cursor channel via mitt.

## Requirements

### Requirement: Global Time Window

The web app SHALL maintain exactly one `GlobalTimeWindow { from, to, mode: 'relative'|'absolute' }` in a zustand store; all queries SHALL receive their time range from this store (or a frame's `time_range_override`). The default value at boot is `{ from: 'now-1h', to: 'now', mode: 'relative' }`.

#### Scenario: Relative window re-resolves on read
- **WHEN** a query reads the time window in relative mode `now-1h..now`
- **THEN** the resolution `from`/`to` are recomputed against the current wall clock at every query issuance (not at window-set time)

#### Scenario: Absolute window stays fixed
- **WHEN** the user picks an absolute window `09:00 – 10:00 UTC`
- **THEN** subsequent queries use those exact instants until the window is changed again

### Requirement: Time Window Picker

Pressing `t` SHALL open a time picker popover next to the status strip with: a preset list (`-5m -15m -1h -6h -24h -7d -30d`), an "Absolute" tab with two ISO inputs, and inline shortcuts `+` / `-` to widen/narrow by 2×. Confirming the picker SHALL update the global window; pressing `Esc` cancels.

#### Scenario: Preset selection
- **WHEN** the user presses `t` and selects `-15m`
- **THEN** the window becomes relative `now-15m..now` and the popover closes

#### Scenario: Absolute requires valid ISO
- **WHEN** the user types an invalid date in the Absolute `from` field
- **THEN** the Confirm button is disabled and an inline error reads `invalid ISO datetime`

### Requirement: Pinned Anchor

A pinned anchor `{ at: iso, label?: string }` SHALL be a single optional instant attached to the global state. Pressing `p` while a chart cursor is hovering SHALL set the anchor to that cursor's `t`; pressing `p` again with no anchor change SHALL unpin. The anchor SHALL render as a vertical 1px `accent` line in every time-aware visualization with a small badge `📌 <hh:mm:ss>` near the top axis.

#### Scenario: Pin from chart cursor
- **WHEN** the user hovers a TimeSeriesPlot at `t=09:42:31` and presses `p`
- **THEN** `anchor.at` becomes `09:42:31`, every chart shows the vertical line at that x, and the badge appears

#### Scenario: Unpin via repeat key
- **WHEN** the anchor is set and the user presses `p` while no chart cursor is hovering
- **THEN** the anchor is cleared and all vertical lines and badges disappear

#### Scenario: Anchor persists across stack
- **WHEN** the user pins an anchor and pushes a new investigation frame
- **THEN** the new frame's visualizations render the same anchor line until that frame sets its own `anchor_override`

### Requirement: Cursor Synchronization

When a user hovers a TimeSeriesPlot or TraceFlame, the cursor `t` SHALL broadcast on a `CursorChannel`; every subscribed visualization on the same view SHALL render its own crosshair at the same `t`, throttled to one update per animation frame.

#### Scenario: Synced crosshair across two charts
- **WHEN** two TimeSeriesPlots A and B are visible and the user moves the mouse over A
- **THEN** B renders a crosshair at the same x as A within one animation frame; both show tooltips of their own value at that t

#### Scenario: Drawer charts only sync within their frame
- **WHEN** a drawer is open with its own charts
- **THEN** cursor events from the drawer broadcast only to subscribers within that drawer's `InvestigationFrame`; main-view charts do not move
