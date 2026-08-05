## ADDED Requirements

### Requirement: Stat presents responsive reduced values

The dashboard engine SHALL reduce every plottable numeric field using the configured calculation and SHALL present the result with the field's display configuration in a responsive BigValue layout.

#### Scenario: Multiple reduced fields

- **WHEN** a Stat panel receives multiple numeric fields
- **THEN** each field is shown in an auto-fitting tile with aligned value and name hierarchy

#### Scenario: Value mapping and text mode

- **WHEN** a reduced value matches a field mapping and `textMode` is configured
- **THEN** the mapped text and color are used and only the text elements allowed by that mode are visually shown

#### Scenario: Sparkline has enough data and space

- **WHEN** `graphMode` is enabled, a field has at least two finite points, and the tile is not compact
- **THEN** a non-interactive sparkline is rendered behind the value without obscuring the primary text

#### Scenario: Percent change starts at zero

- **WHEN** percent change is enabled and the first finite value is zero
- **THEN** no infinite or `NaN` percent change is displayed

### Requirement: Bar Gauge represents independent field ranges

The dashboard engine SHALL render every reduced numeric field as an accessible meter using that field's normalized range and display configuration.

#### Scenario: Value exceeds configured range

- **WHEN** a Bar Gauge value exceeds its configured maximum
- **THEN** the filled bar is clamped to the track while the actual formatted value remains visible

#### Scenario: Horizontal and vertical orientations

- **WHEN** the panel orientation changes between horizontal and vertical
- **THEN** names, values, tracks, and fills are laid out for that orientation without changing data semantics

#### Scenario: Threshold markers enabled

- **WHEN** a field has absolute or percentage thresholds and threshold markers are enabled
- **THEN** normalized threshold boundaries are shown on the track and the active fill uses the resolved display color

### Requirement: Bar Chart models categories and series

The dashboard engine SHALL construct grouped bar data from category fields and numeric series and SHALL render a zero-based quantitative axis in either orientation.

#### Scenario: Tabular category data

- **WHEN** a frame contains a string category field and multiple numeric fields
- **THEN** each row becomes a category and each numeric field becomes a consistently colored series

#### Scenario: Numeric fields without a category

- **WHEN** frames contain numeric fields but no category field
- **THEN** each reduced numeric field becomes a labeled category rather than being discarded

#### Scenario: Positive and negative values

- **WHEN** bar values include both positive and negative numbers
- **THEN** bars extend from a visible zero baseline in the correct direction

#### Scenario: Excessive categories

- **WHEN** prepared bar data contains more than the supported category limit
- **THEN** the most recent categories are retained and the chart remains scrollable and bounded

### Requirement: Heatmap renders a bounded field matrix

The dashboard engine SHALL represent numeric fields as matrix rows, align their samples as columns, and encode finite values against a shared range.

#### Scenario: Multiple numeric series

- **WHEN** multiple numeric fields contain aligned samples
- **THEN** each field is rendered as a named row using the same global intensity scale

#### Scenario: More than 120 samples

- **WHEN** a heatmap row contains more than 120 samples
- **THEN** consecutive samples are aggregated into at most 120 columns using finite-value means and null-only windows remain empty

#### Scenario: Constant values

- **WHEN** all finite heatmap values are equal
- **THEN** cells render with a stable medium intensity and no invalid opacity or division by zero

#### Scenario: Empty numeric data

- **WHEN** no finite numeric sample exists
- **THEN** the shared visualization empty state is rendered

### Requirement: State Timeline represents actual durations

The dashboard engine SHALL convert each non-time field into state segments positioned by time duration, with index-based timing as a fallback.

#### Scenario: Irregular timestamps

- **WHEN** state samples have irregular time gaps
- **THEN** each segment width is proportional to its actual duration rather than the number of samples

#### Scenario: Consecutive equal states

- **WHEN** `mergeEqual` is enabled and adjacent samples have the same formatted state and color
- **THEN** they are rendered as one continuous segment

#### Scenario: No time field

- **WHEN** a frame has state values without a usable time field
- **THEN** the engine assigns monotonically increasing index positions and renders a valid timeline

#### Scenario: State labels in auto mode

- **WHEN** `showValues` is `auto`
- **THEN** text is shown only on segments wide enough to contain it while the complete value remains available through native title text

### Requirement: Chart visualizations adapt and expose accessible summaries

All five chart visualizations SHALL fit the available panel area, preserve essential information in compact layouts, and expose concise non-interactive accessibility semantics.

#### Scenario: Compact panel

- **WHEN** a chart's measured width or height falls below its regular-layout threshold
- **THEN** secondary labels or decoration are reduced before the primary value, scale, or state information is removed

#### Scenario: Assistive technology discovers a chart

- **WHEN** a chart visualization is rendered
- **THEN** it exposes an image or meter semantic with a descriptive accessible name and decorative SVG/DOM geometry is not keyboard focusable

#### Scenario: Light and dark themes

- **WHEN** the active MoleSignal theme changes
- **THEN** chart surfaces, text, tracks, axes, and fallback colors resolve through existing CSS tokens without component-specific hardcoded black or white

### Requirement: Grafana-derived behavior remains isolated and attributable

The implementation SHALL remain independent of Grafana runtime packages and SHALL attribute only the Apache-2.0 source material actually adapted.

#### Scenario: Dependency inspection

- **WHEN** web dependencies and imports are inspected
- **THEN** no `@grafana/*`, Emotion, or tinycolor runtime dependency is required by these visualizations

#### Scenario: Provenance inspection

- **WHEN** a maintainer reads the third-party notice
- **THEN** it identifies Grafana v13.1.0, its pinned commit, the adapted BigValue, BarGauge, and Sparkline source paths, the Apache-2.0 license, and the local modifications

#### Scenario: AGPL source boundary

- **WHEN** the Bar Chart, Heatmap, and State Timeline implementations are inspected
- **THEN** they contain no copied source from Grafana's `public/app/plugins/panel` directories

### Requirement: Existing Dashboard surfaces use the chart registry end to end

The dashboard engine SHALL render registered chart visualizations through the existing Dashboard view, editor live preview, fullscreen panel, and restricted public-share surfaces without a parallel chart runtime.

#### Scenario: Runtime query frames reach a registered chart

- **WHEN** `DashboardRenderer` resolves a panel query into DataFrames and applies transformations and field configuration
- **THEN** `VisualizationRenderer` renders the component registered for that panel's visualization type with the resulting panel data

#### Scenario: Persisted panel has sparse options

- **WHEN** an existing Dashboard stores only a subset of the selected plugin's current options
- **THEN** runtime rendering and the editor resolve the missing values from current plugin defaults while preserving stored overrides

#### Scenario: Editor changes visualization type

- **WHEN** a user changes a panel from one visualization type to another
- **THEN** the target plugin starts with its defaults and only supported same-name option values are carried forward

#### Scenario: Query has not returned chart data

- **WHEN** a registered chart panel is loading without frames or its query fails
- **THEN** the panel renders a concise status or alert state instead of reporting an empty dataset

#### Scenario: Existing Dashboard integration is exercised

- **WHEN** integration tests render a Dashboard containing the registered chart types with an injected query executor
- **THEN** each panel exposes the expected chart or meter semantic through the production `DashboardRenderer` path

#### Scenario: Editor changes a query legend

- **WHEN** a user edits a query's Legend template while query data is already available
- **THEN** the cached DataFrames are relabeled immediately and the chart legend updates without re-executing the data-source query

#### Scenario: Prometheus query selects a Legend mode

- **WHEN** a user opens the Legend control for a metrics query
- **THEN** it offers `Auto`, `Verbose`, and `Custom`, where Auto shows only labels that differ between returned series, Verbose shows all label names and values, and Custom accepts a `{{label_name}}` naming template

#### Scenario: User switches Legend presentation

- **WHEN** a user switches between Auto, Verbose, and Custom or edits the Custom template
- **THEN** the visible series names update from cached DataFrames without entering loading state or re-executing the data-source query

#### Scenario: User selects Legend calculations

- **WHEN** a user opens the time-series Legend values control
- **THEN** it presents the supported calculations as a checkable multi-select with selected values shown as labels instead of requiring a comma-separated string

#### Scenario: Legend calculations update the live preview

- **WHEN** a user selects or clears `Last`, `Min`, `Max`, `Mean`, or `Total`
- **THEN** the persisted `legendStats` array and visible legend columns update immediately without re-executing the data-source query

### Requirement: Dashboard editing happens on the rendered Dashboard

The Dashboard editor SHALL open directly on the production Dashboard canvas and SHALL NOT require a separate Layout page or static panel preview.

#### Scenario: User enters Dashboard edit mode

- **WHEN** a user activates Edit from an existing Dashboard
- **THEN** the edit route renders the Dashboard's live panels and query results through `DashboardRenderer` with editing controls on the same canvas

#### Scenario: User edits a specific panel

- **WHEN** a user activates Edit on a panel from the Dashboard edit canvas
- **THEN** the panel query and visualization editor opens and returning from it restores the live Dashboard edit canvas

#### Scenario: User changes panel placement

- **WHEN** a user drags or resizes a root Dashboard element in edit mode
- **THEN** the live panel remains visible at its preview position and the resulting grid position is committed to editor history when the interaction ends

#### Scenario: User drags a panel by its title bar

- **WHEN** a user presses and drags a non-interactive part of a panel title bar in Dashboard edit mode
- **THEN** the complete title bar acts as the move handle, no standalone drag button is rendered, and title-bar menu controls remain independently operable

#### Scenario: User clicks a panel title without moving it

- **WHEN** a pointer interaction on the title bar does not change the element's grid position
- **THEN** the panel may become selected but no layout history entry is committed

#### Scenario: Editor selection is visible

- **WHEN** an element is selected or keyboard-focused on the edit canvas
- **THEN** selection and focus are communicated with background, text, or icon changes without a ring, box shadow, or high-contrast focus border

#### Scenario: User configures a query variable

- **WHEN** a user edits Query/options for a Dashboard variable
- **THEN** the editor presents a Query type selection and type-specific fields instead of a raw JSON text area

#### Scenario: Existing query variable is opened

- **WHEN** an existing variable stores a `label_values` expression, a classic expression, or an SQL query
- **THEN** the editor infers the matching selection while preserving the resolver-compatible expression, kind, and stream fields

### Requirement: Dashboard configuration uses structured controls

The Dashboard editor SHALL expose domain-specific controls for user configuration and SHALL NOT require users to edit serialized JSON.

#### Scenario: User opens Dashboard settings

- **WHEN** a user opens Dashboard settings
- **THEN** the navigation contains only `General`, `Variables`, `Annotations`, and `Links`, with no editable JSON model page

#### Scenario: User configures annotations or data links

- **WHEN** a user edits annotation events or panel data-link variables
- **THEN** the editor presents typed event fields and key/value rows while preserving compatible persisted records

#### Scenario: User configures transformations

- **WHEN** a user selects a transformation type
- **THEN** its known options are presented as fields, selections, numbers, lists, or mappings instead of a JSON text area

#### Scenario: User configures field behavior

- **WHEN** a user edits thresholds, value mappings, or field override properties
- **THEN** the editor presents addable typed rows and property-specific controls instead of serialized arrays or objects

#### Scenario: Existing extended configuration is edited

- **WHEN** an imported Dashboard record contains additional option keys that the current structured control does not expose
- **THEN** editing a known option preserves those additional keys without requiring a schema migration

#### Scenario: Imported visualization option is not recognized

- **WHEN** an imported visualization contains an unsupported nested option value
- **THEN** the editor reports that the value is preserved and does not expose a raw JSON fallback
