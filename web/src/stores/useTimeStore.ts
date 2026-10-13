import * as React from 'react';
import { useStore } from 'zustand';
import { createStore, type StoreApi } from 'zustand/vanilla';

export type TimeMode = 'relative' | 'absolute';

/**
 * A time expression is either a relative shorthand (`now-1h`, `now`) or an
 * absolute ISO datetime string. We keep them as strings to round-trip through
 * the URL without parsing precision loss.
 */
export type TimeExpr = string;

export interface TimeWindow {
  from: TimeExpr;
  to: TimeExpr;
  mode: TimeMode;
}

export interface Anchor {
  at: string; // ISO datetime
  label?: string;
}

export interface TimeState {
  window: TimeWindow;
  anchor: Anchor | null;
  setWindow: (w: TimeWindow) => void;
  setAnchor: (a: Anchor | null) => void;
  clearAnchor: () => void;
  togglePin: (at: string) => void;
}

export const DEFAULT_WINDOW: TimeWindow = {
  from: 'now-1h',
  to: 'now',
  mode: 'relative',
};

export type TimeStoreApi = StoreApi<TimeState>;

/** A time store of its own: the app has one, and each query tab has one. */
export function createTimeStore(
  initial: { window?: TimeWindow; anchor?: Anchor | null } = {},
): TimeStoreApi {
  return createStore<TimeState>((set, get) => ({
    window: initial.window ?? DEFAULT_WINDOW,
    anchor: initial.anchor ?? null,
    setWindow: (window) => set({ window }),
    setAnchor: (anchor) => set({ anchor }),
    clearAnchor: () => set({ anchor: null }),
    togglePin: (at) => {
      const cur = get().anchor;
      if (cur && cur.at === at) {
        set({ anchor: null });
      } else {
        set({ anchor: { at } });
      }
    },
  }));
}

const appTimeStore = createTimeStore();

/**
 * The time store a subtree reads instead of the app's. A query tab provides its
 * own, so everything rendered in the tab (the range picker, charts, links that
 * carry the window) follows that tab's time and leaves the other tabs alone.
 */
export const TimeScopeContext = React.createContext<TimeStoreApi | null>(null);

/** The store in scope, for reads and writes outside of render. */
export function useTimeStoreApi(): TimeStoreApi {
  return React.useContext(TimeScopeContext) ?? appTimeStore;
}

function useScopedTimeStore<T>(selector: (state: TimeState) => T): T {
  return useStore(useTimeStoreApi(), selector);
}

/**
 * The time window and anchor in scope. As a hook it follows `TimeScopeContext`;
 * `getState` / `setState` / `subscribe` always address the app's own store.
 */
export const useTimeStore = Object.assign(useScopedTimeStore, {
  getState: appTimeStore.getState,
  setState: appTimeStore.setState,
  subscribe: appTimeStore.subscribe,
  getInitialState: appTimeStore.getInitialState,
});

export function sameTimeWindow(a: TimeWindow, b: TimeWindow): boolean {
  return a.from === b.from && a.to === b.to && a.mode === b.mode;
}

export function isDefaultTimeWindow(window: TimeWindow): boolean {
  return sameTimeWindow(window, DEFAULT_WINDOW);
}

/**
 * Writes a time window and anchor into search params the way a URL carries
 * them: `time=from..to` unless it is the default window, and `anchor` if set.
 */
export function writeTimeParams(
  params: URLSearchParams,
  window: TimeWindow,
  anchor: Anchor | null,
): void {
  if (isDefaultTimeWindow(window)) params.delete('time');
  else params.set('time', `${window.from}..${window.to}`);
  if (anchor) params.set('anchor', anchor.at);
  else params.delete('anchor');
}

/**
 * Resolve a TimeWindow against the current wall clock. Relative expressions
 * (`now-Xs/m/h/d`) are recomputed at every call so the result floats forward
 * for live views. Absolute expressions pass through.
 */
export function resolveWindow(window: TimeWindow, now: Date = new Date()): { from: Date; to: Date } {
  return {
    from: resolveExpr(window.from, now),
    to: resolveExpr(window.to, now),
  };
}

const REL = /^now(?:-(\d+)([smhd]))?$/;

export function resolveExpr(expr: TimeExpr, now: Date): Date {
  const m = REL.exec(expr);
  if (m) {
    if (!m[1]) return now;
    const n = Number(m[1]);
    const unit = m[2];
    const ms = unit === 's' ? n * 1000 : unit === 'm' ? n * 60_000 : unit === 'h' ? n * 3_600_000 : n * 86_400_000;
    return new Date(now.getTime() - ms);
  }
  // assume ISO
  const d = new Date(expr);
  if (Number.isNaN(d.getTime())) {
    throw new Error(`invalid time expression: ${expr}`);
  }
  return d;
}

export function formatWindowSummary(window: TimeWindow): string {
  if (window.mode === 'relative') {
    if (window.to === 'now' && window.from.startsWith('now-')) {
      return `Last ${window.from.replace('now-', '')}`;
    }
    return `${window.from} - ${window.to}`;
  }
  // Absolute: render short HH:MM range when same day
  try {
    const f = new Date(window.from);
    const t = new Date(window.to);
    const same = f.toDateString() === t.toDateString();
    const fmt = (d: Date) =>
      d.toISOString().slice(11, 19);
    return same ? `${fmt(f)} - ${fmt(t)} UTC` : `${f.toISOString().slice(0, 16)} - ${t.toISOString().slice(0, 16)}`;
  } catch {
    return `${window.from} - ${window.to}`;
  }
}
