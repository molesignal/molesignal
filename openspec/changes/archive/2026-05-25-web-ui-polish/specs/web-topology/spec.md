## MODIFIED Requirements

### Requirement: Degree-of-Interest Coloring

Each node SHALL be colored by `degree_of_interest = w_err * error_rate + w_p95 * normalize(p95_ms)`; the resulting score `[0,1]` maps to a smooth `green → yellow → red` gradient interpolated via per-component mix (no 5-stop banding). **The red 2px outer ring SHALL be governed by a two-threshold hysteresis: turn on when `err_rate >= 0.05`, and turn off only after `err_rate < 0.045`. The on/off decision is held per-node id in the `useTopologyFlags` zustand store and recomputed when topology data changes; ServiceNode reads the latched flag rather than computing the ring locally.**

#### Scenario: Healthy service is green

- **WHEN** a node has `error_rate = 0.001` and `p95_ms = 80`
- **THEN** its fill resolves near `green`; no red ring is drawn

#### Scenario: High error gets red ring

- **WHEN** a node has `error_rate = 0.07`
- **THEN** a 2px red ring is drawn outside the node circle in addition to the score-based fill

#### Scenario: DOI fills are smooth

- **WHEN** two adjacent nodes have err_rate = 0.04 and 0.06 respectively (p95 equal)
- **THEN** their fill colors interpolate smoothly along the gradient (no banding)

#### Scenario: Red ring hysteresis prevents flicker

- **WHEN** a node's `err_rate` oscillates 0.049 ↔ 0.051 every poll
- **THEN** the red ring is shown continuously (does not flicker on/off)
- **AND** when `err_rate` drops to 0.044, the ring is removed
