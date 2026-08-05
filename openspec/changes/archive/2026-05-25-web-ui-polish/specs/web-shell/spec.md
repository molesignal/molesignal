## MODIFIED Requirements

### Requirement: Theme And Density Tokens

The shell SHALL expose CSS custom properties for exactly nine semantic colors (`bg`, `surface`, `primary`, `accent`, `red`, `green`, `yellow`, `blue`, `purple`) per theme (`dark`, `light`), and two density modes (`compact`, `comfortable`) controlling row height and padding tokens, with `compact` as the default for authenticated routes. **All foreground/background pairs that may be combined at runtime MUST meet WCAG 2.1 AA contrast (4.5:1 for body text, 3:1 for large/UI elements), as verified by `pnpm -C web a11y:contrast`.**

#### Scenario: Theme tokens are limited

- **WHEN** a developer inspects `:root`
- **THEN** the only chrome color CSS variables defined are the nine semantic names (per theme) plus their `*-muted` and `*-bg` variants; no additional palette colors are exported

#### Scenario: Density default

- **WHEN** an authenticated user opens any route for the first time and has not set a density preference
- **THEN** the body element carries `data-density="compact"` and the row height token resolves to `28px`; switching to comfortable yields `36px`

#### Scenario: All active token pairs meet AA contrast

- **WHEN** `pnpm -C web a11y:contrast` runs in CI after token edits
- **THEN** it exits 0 and prints zero `FAIL` lines for both dark and light themes

## ADDED Requirements

### Requirement: StatusStrip Spacing Standard

The top status strip SHALL use a 4px `•` dot as the section separator between org / cluster / window / `⌘K` hint / avatar, with 16px gap between sections (replacing the previous `|` + 12px). The anchor (`📌 hh:mm:ss`) element SHALL reserve `min-width: 12ch` so its appearance does not shift neighbor sections when the time changes.

#### Scenario: Status strip layout is byte-stable on time tick

- **WHEN** the visual baseline `login-*.png` is regenerated at `2026-05-23T10:00:00Z` vs the same fixture at `10:00:30Z`
- **THEN** non-anchor pixels are identical (no neighbor reflow)
