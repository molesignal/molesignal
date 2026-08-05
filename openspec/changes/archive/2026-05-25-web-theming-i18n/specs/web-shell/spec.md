## ADDED Requirements

### Requirement: StatusStrip Settings Dropdown

The StatusStrip SHALL include a Settings (gear) trigger to the left of the avatar; the dropdown SHALL contain four sections — Theme / Palette / Density / Language — surfacing every option per section as a checkable item. Existing scattered toggles (sun-moon icon, palette `Toggle theme` / `Toggle density` static actions) SHALL keep working but are no longer the primary affordance.

#### Scenario: Gear opens the unified settings dropdown

- **WHEN** the user clicks the gear icon in the StatusStrip
- **THEN** a dropdown opens listing Theme (dark, light), Palette (default, high-contrast, warm), Density (compact, comfortable), Language (en, zh-CN)
- **AND** each section's active option carries a leading checkmark

#### Scenario: Legacy theme toggle still works

- **WHEN** the user clicks the sun/moon icon (legacy single-purpose toggle)
- **THEN** `useThemeStore.theme` flips between dark and light
- **AND** the gear dropdown's Theme section reflects the new value

### Requirement: No Hardcoded Light-Mode Black

JSX or inline styles in `web/src/**/*.tsx` SHALL NOT use any of `text-black`, `bg-black`, `border-black`, `color: #000`, or `color: black`. All color references go through tokens (`text-foreground`, `text-tx-*`, `bg-bg`, `bg-surface`, `border-border`, etc.). A lint or grep gate enforces this on PR.

#### Scenario: Grep gate catches a regression

- **WHEN** a contributor adds `className="text-black"` to a new component
- **AND** runs `pnpm -C web lint`
- **THEN** an ESLint rule (or scripted grep step) reports the offending line and exits non-zero
