## MODIFIED Requirements

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
