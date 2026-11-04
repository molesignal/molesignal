import type { TraceWindow } from '@/api/web';

/** A trace the drawer shows: which, when its spans began, and the span to begin on. */
export interface DrawerTrace {
  traceId: string;
  window?: TraceWindow | undefined;
  spanId?: string | null | undefined;
}

/**
 * What the drawer holds. It keeps its traces while it closes, so it does not
 * empty as it goes; following a link from one trace into another stacks the
 * one it came from, to go back to.
 */
export interface TraceDrawerState {
  stack: DrawerTrace[];
  open: boolean;
}

/** A minute before a trace began. */
const BEFORE_MICROS = 60 * 1_000_000;
/** A quarter of an hour after it ended: spans that arrive late begin after the trace did. */
const AFTER_MICROS = 15 * 60 * 1_000_000;

/** Where a listed trace's spans are looked for: around the moment it began and for as long as it ran. */
export function listedTraceWindow(startNs: number, durationMs: number, nowMicros = Date.now() * 1_000): TraceWindow | undefined {
  if (!Number.isFinite(startNs) || startNs <= 0) return undefined;

  const start = Math.floor(startNs / 1_000);
  const end = start + Math.ceil(Math.max(0, durationMs) * 1_000);

  return { from: start - BEFORE_MICROS, to: Math.max(start + 1, Math.min(nowMicros, end + AFTER_MICROS)) };
}

export function openedTrace(trace: DrawerTrace): TraceDrawerState {
  return { stack: [trace], open: true };
}

/** A link followed: the trace it leads to goes on top, the one it came from stays under it. */
export function followedTrace(state: TraceDrawerState, trace: DrawerTrace): TraceDrawerState {
  return { stack: [...state.stack, trace], open: true };
}

/** Back to the trace a link was followed from. */
export function backTrace(state: TraceDrawerState): TraceDrawerState {
  return state.stack.length > 1 ? { ...state, stack: state.stack.slice(0, -1) } : state;
}

export function currentTrace(state: TraceDrawerState | null): DrawerTrace | null {
  return state?.stack[state.stack.length - 1] ?? null;
}
