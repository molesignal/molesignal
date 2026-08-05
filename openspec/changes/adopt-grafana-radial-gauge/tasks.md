## 1. Source Boundary and Module Setup

- [x] 1.1 Add a third-party notice pinning the Apache-2.0 Grafana v13.1.0 RadialGauge source and documenting MoleSignal's adaptations
- [x] 1.2 Create the dedicated `visualizations/gauge` module structure without adding any `@grafana/*` dependency

## 2. Gauge Geometry

- [x] 2.1 Implement stable range normalization, clamped value ratios, and radial SVG arc path generation
- [x] 2.2 Implement absolute and percentage threshold interval normalization
- [x] 2.3 Add focused unit tests for range, arc, clamp, and threshold boundary behavior

## 3. Radial Gauge Presentation

- [x] 3.1 Implement the responsive SVG radial gauge with track, active arc, optional threshold ring, and labels
- [x] 3.2 Add compact-layout behavior and a single descriptive non-interactive image semantic
- [x] 3.3 Add component tests for labels, threshold visibility, compact mode, and accessibility

## 4. Dashboard Engine Integration

- [x] 4.1 Implement the gauge data adapter for numeric field selection, reduction, field formatting, range, and empty state
- [x] 4.2 Register the extracted gauge component with backward-compatible defaults and remove the old CSS gauge implementation
- [x] 4.3 Add integration tests for latest-value calculation, field formatting/mapping, out-of-range values, and empty data

## 5. Verification

- [x] 5.1 Run focused gauge tests and fix all failures
- [x] 5.2 Run the web TypeScript typecheck and touched-file lint, documenting any unrelated pre-existing failures
