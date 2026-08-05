# Web Investigation Stack

## Purpose

Right-side drawer stack of up to 6 InvestigationFrames with per-frame Esc/back/forward, pin/unpin, drop-oldest overflow, URL serialization (gzip+base64; >4 KiB spills to blob endpoint), and cascade-pop on outside click.
## Requirements
### Requirement: Stack Data Model

An investigation stack SHALL be an ordered list of frames `[F0, F1, ..., Fn]` where each frame is `{ id, kind, params, time_range_override?, anchor_override?, parent_frame_id?, created_at }`. `kind` is one of `trace`, `log`, `metric`, `host`, `service`, `incident`, `sql`, `promql`, `dashboard_panel`, `saved_view`. The stack SHALL hold at most 6 active frames; pushing a 7th SHALL drop the oldest unpinned frame.

#### Scenario: Push creates child frame
- **WHEN** a frame `F1 (trace, trace_id=abc)` is on top and the user clicks a `host=ip-10-0-1-2` link inside it
- **THEN** a new frame `F2 (host, host=ip-10-0-1-2, parent_frame_id=F1.id, time_range_override=F1.window)` is pushed; F1 remains in the stack

#### Scenario: Cap enforces drop-oldest
- **WHEN** the stack already has 6 frames and a 7th push occurs
- **THEN** the oldest frame whose `pinned` flag is false is removed; if all 6 are pinned, the push is refused and a toast says `Stack full — unpin a frame to add another`

### Requirement: Navigation

The stack SHALL support `back` (re-show the previous frame as the top after preserving the popped frame in a forward buffer), `forward` (re-push the most recently popped frame), `pop` (remove the top frame and clear the forward buffer), and `reset` (clear all frames and forward buffer). `⌘[` triggers `back`, `⌘]` triggers `forward`, `Esc` (at `drawer` scope) triggers `pop`.

#### Scenario: Back and forward are inverse
- **WHEN** the user is at top frame F3 and presses `⌘[` then `⌘]`
- **THEN** the top frame returns to F3; the forward buffer is empty after the second key

#### Scenario: New push clears forward buffer
- **WHEN** the user pressed `⌘[` (F3 popped to buffer) and then pushes a new frame F4
- **THEN** the forward buffer is cleared; subsequent `⌘]` does nothing

### Requirement: Visual Stacking

Frames `F1..Fn` SHALL render as right-aligned drawers, each 720px wide, stacked left-to-right with a 24px offset and 8px gap so the user sees the right edges of all lower drawers; only the top drawer is fully visible and focused. The main view behind the stack SHALL dim with a 30% black overlay only when at least one drawer is open.

#### Scenario: Stacked offset visible
- **WHEN** three frames are on the stack
- **THEN** the rightmost (top) drawer occupies its full 720px; the second drawer's right edge protrudes 32px to the left of the top; the third protrudes 64px

#### Scenario: Click on lower drawer
- **WHEN** the user clicks the visible right edge of a lower drawer
- **THEN** the stack pops down to that drawer (frames above it move to the forward buffer in reverse order, popping cascade)

### Requirement: URL Serialization

The full stack (frames + anchor + global time + filters) SHALL serialize into URL parameters: `?stack=<base64-json>` (frames array), `?anchor=<iso>`, `?time=<iso>..<iso>`, `?filters=<base64-json>`. Pasting the URL into a new tab SHALL reproduce the same stack and views.

#### Scenario: Round-trip equality
- **WHEN** a user copies the URL with `y`, opens it in a new browser
- **THEN** the rendered stack frames (kinds and params), time window, and pinned anchor match the source byte-for-byte after deserialization

#### Scenario: Stack size in URL
- **WHEN** a stack with 6 frames is serialized
- **THEN** the resulting URL is at most 4096 bytes; if a frame's params payload would exceed this, the offending frame's `params` is stored under a server-side `investigation_blob` and the URL holds an opaque `blob_id` reference

### Requirement: Frame Context Propagation

When a new frame is pushed, the source frame SHALL pass a `CorrelationContext { time_range, trace_id?, service?, host?, severity?, filters[] }` to the new frame's loader; the loader (per `web-correlation`) decides how to translate that context into a query.

#### Scenario: Trace span to log frame
- **WHEN** the user clicks the `view logs` action on a span with `service=checkout, trace_id=t1, time=09:42:31`
- **THEN** the pushed log frame's `params` contains `filters: [{ field: 'service.name', op: '=', value: 'checkout' }, { field: 'trace_id', op: '=', value: 't1' }]` and `time_range_override: [09:42:00, 09:43:00]`

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

