## ADDED Requirements

### Requirement: Three-Axis UI Preferences

The web app SHALL expose three orthogonal UI preference axes: `theme` (`dark | light`), `palette` (`default | high-contrast | warm`), and `density` (`compact | comfortable`). Each axis is persisted in localStorage and reflected on `<html>` as `data-theme` / `data-palette` / `data-density`. The Settings dropdown in StatusStrip SHALL surface a one-click switch for each axis.

#### Scenario: Axes are independent

- **WHEN** the user picks `theme=dark` and `palette=high-contrast`
- **THEN** `<html>` carries both `data-theme="dark"` and `data-palette="high-contrast"`
- **AND** the tokens.css cascade applies both rule sets

#### Scenario: Settings dropdown lists each axis

- **WHEN** the user opens the Settings gear in the StatusStrip
- **THEN** the dropdown shows four sections in order: Theme, Palette, Density, Language
- **AND** the currently active option in each section carries `data-current="true"`

### Requirement: Palette Contrast Gate

Every palette (`default`, `high-contrast`, `warm`) SHALL pass `pnpm -C web a11y:contrast` for both `dark` and `light` themes. Adding a new palette without lifting all of its pairs to WCAG AA SHALL fail CI.

#### Scenario: New palette fails CI when below AA

- **WHEN** a contributor adds `tokens-experimental.css` with a fg/bg pair scoring 3.8:1
- **AND** runs `pnpm -C web a11y:contrast`
- **THEN** the script exits non-zero, lists the failing pair, and CI blocks the PR
