## 1. Shared Visualization Foundation

- [x] 1.1 Extend the third-party notice for the pinned Apache-2.0 BigValue, BarGauge, and Sparkline sources and document the AGPL exclusion
- [x] 1.2 Create shared visualization props, empty state, and measured-container primitives
- [x] 1.3 Extract finite reduction, stable range, threshold, timestamp, and deterministic color helpers
- [x] 1.4 Refactor the radial gauge to reuse shared range and threshold helpers without behavior regressions
- [x] 1.5 Add focused shared-helper tests for reductions, ranges, thresholds, timestamps, and colors

## 2. Stat Visualization

- [x] 2.1 Implement the Stat data model with field reduction, display formatting, sparkline values, and finite percent change
- [x] 2.2 Implement the responsive BigValue and Sparkline presentation with text and color modes
- [x] 2.3 Add Stat model, component, compact-layout, mapping, and accessibility tests

## 3. Bar Gauge Visualization

- [x] 3.1 Implement per-field Bar Gauge values, normalized ranges, display colors, and threshold markers
- [x] 3.2 Implement accessible horizontal and vertical Bar Gauge layouts
- [x] 3.3 Add Bar Gauge range, orientation, threshold, mapping, and meter-semantics tests

## 4. Bar Chart Visualization

- [x] 4.1 Implement category/series preparation, deterministic series colors, category limiting, and zero-inclusive domains
- [x] 4.2 Implement grouped vertical and horizontal SVG bar geometry with axes, labels, and native titles
- [x] 4.3 Add Bar Chart tabular, fallback, negative-value, orientation, limit, and accessibility tests

## 5. Heatmap Visualization

- [x] 5.1 Implement bounded series-by-sample heatmap matrix preparation and finite-window aggregation
- [x] 5.2 Implement token-based heatmap cells, labels, empty values, and accessible summary
- [x] 5.3 Add Heatmap matrix, aggregation, constant-range, scheme, and empty-state tests

## 6. State Timeline Visualization

- [x] 6.1 Implement timestamp normalization, duration segments, fallback timing, equal-state merging, and legend extraction
- [x] 6.2 Implement proportional timeline rows, auto value labels, stable colors, axis labels, and accessible summary
- [x] 6.3 Add State Timeline irregular-time, merge, fallback, label-mode, mapping, and accessibility tests

## 7. Registry Integration and Cleanup

- [x] 7.1 Register all five extracted components with backward-compatible and newly documented defaults
- [x] 7.2 Remove the five legacy inline implementations and obsolete numeric/state helpers from `visualizations.tsx`
- [x] 7.3 Extend generic option choices for new chart modes without changing the persisted schema version
- [x] 7.4 Add registry integration coverage for every chart type and its default options

## 8. Verification

- [x] 8.1 Run all focused dashboard visualization tests and fix failures
- [x] 8.2 Run web TypeScript typecheck and touched-file lint
- [x] 8.3 Audit production file lengths, dependency imports, third-party provenance, and OpenSpec completion

## 9. Existing Dashboard Integration

- [x] 9.1 Resolve sparse persisted visualization options against plugin defaults and preserve stored overrides
- [x] 9.2 Cleanly transition options when the Dashboard editor changes visualization type
- [x] 9.3 Add shared loading and error states to the production `VisualizationRenderer` path
- [x] 9.4 Add production `DashboardRenderer` integration coverage for all registered chart types

## 10. Integration Verification

- [x] 10.1 Run dashboard visualization and DashboardRenderer integration tests
- [x] 10.2 Run Web TypeScript typecheck, touched-file lint, and final OpenSpec audit

## 11. In-place Dashboard Editing

- [x] 11.1 Remove the Layout/Panel surface switch and make the panel query parameter the only secondary editor state
- [x] 11.2 Replace the static Layout preview with the production DashboardRenderer and live query/chart path
- [x] 11.3 Add root-grid selection, drag, resize, keyboard nudge, duplicate, remove, and panel-edit controls around rendered elements
- [x] 11.4 Keep the contextual element inspector and editor history/save flow connected to the in-place canvas
- [x] 11.5 Remove obsolete fake preview code and update Dashboard/Panel editor copy in both locales

## 12. In-place Editing Verification

- [x] 12.1 Add focused tests for edit controls, live chart rendering, selection, and panel-open behavior
- [x] 12.2 Run focused dashboard tests, Web TypeScript typecheck, touched-file lint, file-length, focus-style, and OpenSpec audits

## 13. Panel Title-bar Dragging

- [x] 13.1 Remove the standalone panel move button and delegate move pointer interactions to the complete panel title bar
- [x] 13.2 Exclude title-bar menu controls and avoid history commits when the pointer does not change grid placement
- [x] 13.3 Update focused edit-mode tests and rerun Dashboard TypeScript, lint, and OpenSpec validation

## 14. Editor Time Range and Grafana Legend

- [x] 14.1 Add the shared dashboard time-range picker to Dashboard and Panel edit mode
- [x] 14.2 Expose List, Table, and Hidden as explicit Grafana-compatible legend modes
- [x] 14.3 Connect legend placement and calculation options to the production time-series renderer

## 15. Editor Time Range and Legend Verification

- [x] 15.1 Add focused editor, registry, and time-series rendering coverage
- [x] 15.2 Run Dashboard tests, Web TypeScript, touched-file lint, focus-style, file-length, and OpenSpec audits

## 16. Live Query Legend Editing

- [x] 16.1 Exclude presentation-only Legend templates from dashboard query cache identity
- [x] 16.2 Relabel cached DataFrames from the current Legend template without entering loading state
- [x] 16.3 Avoid rebuilding the uPlot instance when only a series display name changes
- [x] 16.4 Add focused integration coverage and rerun Dashboard, TypeScript, lint, and OpenSpec validation

## 17. Grafana Query Legend Modes

- [x] 17.1 Replace the metrics query Legend text field with Grafana-compatible Auto, Verbose, and Custom modes
- [x] 17.2 Apply Auto unique-label, Verbose full-label, and Custom template names to cached metric DataFrames
- [x] 17.3 Default new metrics queries to Auto while preserving legacy empty Legend as Verbose
- [x] 17.4 Add focused editor and presentation coverage and rerun Dashboard, TypeScript, lint, i18n, and OpenSpec validation

## 18. Grafana Legend Values Picker

- [x] 18.1 Replace the comma-separated Legend stats input with a checkable multi-select that shows selected calculations as labels
- [x] 18.2 Present supported calculations as Last, Min, Max, Mean, and Total while preserving the existing `sum` persistence value
- [x] 18.3 Add focused editor and renderer coverage and rerun Dashboard, TypeScript, lint, i18n, and OpenSpec validation

## 19. Structured Variable Query Selector

- [x] 19.1 Replace the Query/options JSON editor with a Query type selection and type-specific fields
- [x] 19.2 Preserve existing label-values, classic-query, and SQL variable records without a schema migration
- [x] 19.3 Add focused compatibility and interaction coverage and rerun Dashboard, TypeScript, lint, i18n, and OpenSpec validation

## 20. Fully Structured Dashboard Configuration

- [x] 20.1 Remove the Dashboard JSON model tab and keep settings navigation limited to General, Variables, Annotations, and Links
- [x] 20.2 Replace annotation event queries and data-link variables with structured collection editors
- [x] 20.3 Replace transformation option JSON with type-specific fields while preserving unknown persisted keys
- [x] 20.4 Replace visualization fallback, threshold, value-mapping, and override-property JSON with typed editors or preservation notices
- [x] 20.5 Add focused interaction and compatibility coverage and rerun Dashboard, TypeScript, lint, i18n, focus-style, file-length, and OpenSpec validation
