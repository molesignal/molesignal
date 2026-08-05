## MODIFIED Requirements

### Requirement: Result Item Contract

Every palette result item SHALL render with: a leading 16px icon (lucide-react), a primary label, an optional muted subtitle (e.g., stream name for a saved view), a right-aligned kind chip (`stream`, `service`, `incident`, `dashboard`, `saved_view`, `alert`, `action`), and a keyboard shortcut hint if the action has one. Selected row SHALL have a 2px `var(--accent)` left border and a `var(--accent-bg)` background at 12% alpha. **In `compact` density mode, subtitle SHALL truncate with ellipsis after a single line; in `comfortable` density, subtitle MAY wrap to a second line before truncating.**

#### Scenario: Required fields present

- **WHEN** any result is rendered
- **THEN** the row contains icon + label + kind chip at minimum; subtitle and shortcut hint are optional and may be absent

#### Scenario: Selected row is unambiguous

- **WHEN** the user navigates with `↓` / `↑` (or `j` / `k` when scope is `palette`)
- **THEN** exactly one row carries the selected state (2px accent border on its left edge + 12%-alpha accent-bg fill), and the list scrolls to keep that row in the viewport center band

#### Scenario: Compact mode does not clip kind chip

- **WHEN** the palette is open in `compact` density and a result item's subtitle is 80+ characters
- **THEN** the kind chip on the right is still fully visible
- **AND** the subtitle is ellipsised at the row's available width with `…`

#### Scenario: Selected row highlight

- **WHEN** a row is selected (via `data-selected="true"` cmdk attribute)
- **THEN** a 2px `var(--accent)` left border is visible
- **AND** the row background gets `var(--accent-bg)` at 12% alpha
