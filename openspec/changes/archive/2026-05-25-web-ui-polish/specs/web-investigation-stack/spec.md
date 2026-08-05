## ADDED Requirements

### Requirement: Drawer Cascade Animation

When a drawer is pushed onto or popped off the investigation stack, it SHALL animate slide-in / slide-out from the right edge at 200ms ease-out. Cascade-pop (clicking the 32px exposed strip of a lower drawer) SHALL animate each intermediate drawer's removal sequentially with 50ms stagger so the user can perceive the unwinding. When the OS reports `prefers-reduced-motion: reduce`, animation durations SHALL collapse to ~0 so the transitions are effectively instantaneous.

#### Scenario: Push animation

- **WHEN** a new frame is pushed via `useInvestigationStack.push(...)`
- **THEN** the drawer transitions from `translateX(100%)` to `translateX(0)` over 200ms
- **AND** the 30% black overlay fades in concurrently

#### Scenario: Cascade pop staggers

- **WHEN** the user clicks the right-edge strip of drawer index 0 while drawers 0/1/2/3 are open
- **THEN** drawer 3 slides out first, then drawer 2 after 50ms, then drawer 1 after 50ms
- **AND** the stack reaches `[drawer 0]` at ~350ms total

#### Scenario: Reduced motion honored

- **WHEN** the browser reports `prefers-reduced-motion: reduce`
- **AND** a drawer is pushed or popped
- **THEN** the slide animation completes within one frame (no perceptible motion)
