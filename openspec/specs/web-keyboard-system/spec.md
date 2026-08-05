# Web Keyboard System

## Purpose

Global scope-stack keyboard controller: capture-phase keydown router, per-scope binding registry, 800ms chord state machine, focus-ring tokens, and an opt-in help overlay rendered from the live binding map.

## Requirements

### Requirement: Global Keymap

The web app SHALL bind global keys with the following exact mapping at the `global` scope: `⌘K` open palette, `Esc` pop current scope, `⌘[` investigation stack back, `⌘]` investigation stack forward, `g s`, `g a`, `g d`, `g t`, `g l` route to `/services`, `/alerts/incidents`, `/dashboards`, `/investigate` (traces preset), `/investigate` (logs preset) respectively, `t` open time window picker, `p` pin / unpin current cursor as anchor, `y` copy current investigation URL, `?` open keyboard help overlay, `j`/`k` move down/up in the currently focused list, `Enter` activate the focused row.

#### Scenario: Two-key sequences time out
- **WHEN** the user presses `g` and waits >800ms before the next key
- **THEN** the pending sequence is dropped; pressing a follow-up key after the timeout starts a new sequence

#### Scenario: Modifier keys are exact
- **WHEN** the user presses `⌘[`
- **THEN** the stack goes back exactly one frame; `⌥[` or `⌃[` does nothing

#### Scenario: Help overlay
- **WHEN** the user presses `?` outside any text input
- **THEN** an overlay opens listing every global binding plus the bindings exposed by the current scope; pressing `?` again or `Esc` closes it

### Requirement: Scope Stack

A scope stack SHALL track which set of bindings is active. Scopes are pushed by `palette` open, `drawer` open, `chart-brush` engagement, `editor` focus, and `help-overlay` open; the top scope's bindings shadow lower scopes. `Esc` SHALL always pop the top scope (closing the corresponding UI element).

#### Scenario: Drawer Esc pops one layer
- **WHEN** a `trace` drawer is on top with an inner `chart-brush` engaged
- **THEN** pressing `Esc` once releases the brush; pressing `Esc` again closes the trace drawer; pressing `Esc` a third time pops the next investigation frame below

#### Scenario: Editor scope swallows j/k
- **WHEN** focus is inside the SQL or PromQL editor (`editor` scope on top)
- **THEN** `j` and `k` insert characters as usual and do not move the list selection

### Requirement: Focus Ring And A11y

Every keyboard-actionable element SHALL display a 2px focus ring using the `accent` color when reached by `Tab`, `j/k`, or programmatic focus, and SHALL have an accessible name (aria-label or visible text). Color contrast for focus ring against background SHALL meet WCAG 2.1 AA (>= 3:1 for non-text).

#### Scenario: Focus visible on j/k navigation
- **WHEN** the user moves the selection in a list using `j` or `k`
- **THEN** the focused row carries the 2px accent ring and is announced via `aria-activedescendant`

#### Scenario: Skip-to-content
- **WHEN** the user presses `Tab` immediately after page load
- **THEN** the first focus stop is a visually-hidden "Skip to content" link that, on activation, moves focus to the main region

### Requirement: Binding Registry API

The shell SHALL expose `registerBindings(scope, bindings)` and `unregisterBindings(handle)` so feature modules can register their own keys; bindings declared per scope SHALL only be active when that scope is at the top of the stack.

#### Scenario: Drawer registers its own bindings
- **WHEN** a trace drawer mounts and calls `registerBindings('drawer', { 'shift+s': openSearch })`
- **THEN** pressing `Shift+S` inside the drawer opens its search; the same combination outside the drawer does nothing

#### Scenario: Duplicate binding rejected
- **WHEN** two modules register the same key at the same scope in the same session
- **THEN** the second `registerBindings` call returns an error and the first registration wins; the help overlay shows the binding once
