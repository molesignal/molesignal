## ADDED Requirements

### Requirement: Gauge renders a configured numeric field

The dashboard engine SHALL render the first plottable numeric field as an SVG radial gauge using the panel's configured calculation and the field's existing display configuration.

#### Scenario: Render the latest numeric value

- **WHEN** a gauge panel receives a data frame containing a numeric field and uses the `last` calculation
- **THEN** the gauge displays the latest finite value, the field display name, and the value formatted with the field's unit, decimals, and value mappings

#### Scenario: No plottable numeric value

- **WHEN** a gauge panel receives no finite numeric value
- **THEN** it displays the dashboard engine's empty-state message instead of an invalid SVG path or `NaN`

### Requirement: Gauge range is numerically stable

The dashboard engine SHALL derive a stable gauge range from field configuration and SHALL clamp only the rendered arc while preserving the actual formatted value.

#### Scenario: Value is outside the configured range

- **WHEN** a value is below `min` or above `max`
- **THEN** the activity arc stops at the nearest end of the gauge and the displayed text retains the actual value

#### Scenario: Minimum and maximum are equal

- **WHEN** configured `min` and `max` are equal
- **THEN** the engine expands the range by a non-zero amount and renders finite SVG geometry

#### Scenario: Minimum is greater than maximum

- **WHEN** configured `min` is greater than configured `max`
- **THEN** the engine normalizes the range into ascending order before calculating the activity arc

### Requirement: Gauge visualizes field thresholds

The dashboard engine SHALL use the field threshold configuration for the active value color and SHALL support an optional threshold interval ring.

#### Scenario: Absolute thresholds

- **WHEN** a field defines absolute threshold steps and threshold markers are enabled
- **THEN** the gauge draws ordered interval arcs across the normalized range using the configured step colors

#### Scenario: Percentage thresholds

- **WHEN** a field defines percentage threshold steps
- **THEN** threshold values are converted against the normalized gauge range before interval arcs and the active color are resolved

#### Scenario: Threshold labels are disabled

- **WHEN** `showThresholdLabels` is false
- **THEN** no visual threshold boundary labels are drawn while threshold colors remain effective

### Requirement: Gauge adapts to panel size

The gauge SHALL scale within the available panel area without overflowing and SHALL prioritize the primary value in compact layouts.

#### Scenario: Regular panel height

- **WHEN** the available height meets the regular-layout threshold
- **THEN** the gauge may show the formatted value, field name, range labels, and enabled threshold labels

#### Scenario: Compact panel height

- **WHEN** the available height is below the regular-layout threshold
- **THEN** the gauge hides secondary visual labels while retaining the primary value and complete accessible name

### Requirement: Gauge exposes accessible semantics

The gauge SHALL expose a single non-interactive image semantic whose accessible name describes the field, formatted value, and normalized range.

#### Scenario: Assistive technology reads a gauge

- **WHEN** the radial gauge is rendered
- **THEN** assistive technology can discover one image role with a descriptive accessible name and does not encounter focusable decorative paths

### Requirement: Grafana-derived code remains isolated and attributable

The implementation SHALL remain independent of Grafana runtime packages and AGPL panel code, and SHALL record the exact Apache-2.0 upstream source and local modifications.

#### Scenario: Frontend dependencies are installed

- **WHEN** the web application dependencies are resolved
- **THEN** no `@grafana/*` runtime dependency is required by the radial gauge

#### Scenario: Third-party provenance is inspected

- **WHEN** a maintainer reviews the repository's third-party notice
- **THEN** it identifies Grafana v13.1.0, the Apache-2.0 RadialGauge source directory, the license, and the fact that the implementation was adapted for MoleSignal
