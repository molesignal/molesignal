## ADDED Requirements

### Requirement: Topology Data Loading

The topology view SHALL fetch the service graph via `GET /api/v1/web/topology?from=&to=` returning `{ nodes: Node[], edges: Edge[] }` where `Node = { id, name, error_rate, p95_ms, rps, span_count }` and `Edge = { source, target, rps, err_rate, p95_ms }`; the response SHALL be cached per `(org, from, to)` for 30 seconds in `@tanstack/react-query`.

#### Scenario: 200 node graph fits budget
- **WHEN** the response contains 200 nodes and 600 edges
- **THEN** the initial render (including layout) completes within 600ms on a modern laptop

#### Scenario: Empty graph
- **WHEN** the response is `{ nodes: [], edges: [] }`
- **THEN** the view shows an empty state `No service traffic in window` and offers a button `Widen time window`

### Requirement: React Flow Base

The view SHALL be built on `reactflow ^11` with a custom node renderer (circle node with service name label below) and a custom edge renderer (curved path with a rotating label band). React Flow's MiniMap, Controls, and Background panels SHALL be enabled.

#### Scenario: MiniMap interactive
- **WHEN** the user clicks any point in the MiniMap
- **THEN** the main viewport pans to that area; the current viewport rectangle in the MiniMap updates accordingly

#### Scenario: Background grid visible at light theme
- **WHEN** the theme is `light`
- **THEN** the Background panel renders dots at 16px spacing with `--surface-muted` color; in `dark` theme the dots use `--surface` color

### Requirement: Force Layout

Initial node positions SHALL be computed by a one-shot force-directed layout (using a lightweight algorithm such as `d3-force` running for at most 300 ticks) on the first render; the result SHALL be cached in the zustand store keyed by `(graph_hash, viewport_size)` so subsequent re-renders are instant.

#### Scenario: Layout cached across re-renders
- **WHEN** the user navigates away from the topology view and back within the same window
- **THEN** the second render reuses the cached positions and skips the force ticks; total time to first paint < 60ms

#### Scenario: Layout recomputes when graph changes
- **WHEN** the underlying topology response changes (new nodes/edges)
- **THEN** the force simulation runs again; the previous cache entry is evicted

### Requirement: Degree-of-Interest Coloring

Each node SHALL be colored by `degree_of_interest = w_err * error_rate + w_p95 * normalize(p95_ms)`; the resulting score `[0,1]` maps to a 5-stop ramp `green → yellow → red`. Nodes with `error_rate >= 0.05` SHALL additionally carry a red 2px outer ring regardless of score.

#### Scenario: Healthy service is green
- **WHEN** a node has `error_rate = 0.001` and `p95_ms = 80`
- **THEN** its fill resolves near `green`; no red ring is drawn

#### Scenario: High error gets red ring
- **WHEN** a node has `error_rate = 0.07`
- **THEN** a 2px red ring is drawn outside the node circle in addition to the score-based fill

### Requirement: Edge Label Rotation

Each edge SHALL display a single rotating label that cycles every 3 seconds through `RPS`, `err%`, `p95`; hovering the edge SHALL pause rotation and show all three values stacked. Clicking the edge SHALL push a `service_to_service` investigation frame.

#### Scenario: Rotation cycle
- **WHEN** an edge has `rps = 1240, err_rate = 0.012, p95_ms = 240`
- **THEN** the label shows `1.2k rps` for 3s, then `1.2% err` for 3s, then `240ms p95` for 3s, then loops

#### Scenario: Hover pauses
- **WHEN** the user hovers an edge
- **THEN** rotation stops and the label expands to a 3-line tooltip showing all three values; on hover-out, rotation resumes from the previously displayed value

### Requirement: Viewport Culling

Nodes and edges fully outside the current React Flow viewport (with a 20% halo) SHALL NOT participate in custom renderer effects (label rotation timers, hover handlers). Only React Flow's own DOM nodes remain mounted; rotation timers SHALL be paused for off-screen edges to keep idle CPU < 1% on a 200-node graph.

#### Scenario: Off-screen timers paused
- **WHEN** 90% of edges scroll out of view
- **THEN** the number of active label-rotation intervals shrinks proportionally; CPU usage measured over 10s in idle stays under 1% on a Mac M1
