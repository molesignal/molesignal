import { create } from 'zustand';

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

interface TimeState {
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

export const useTimeStore = create<TimeState>((set, get) => ({
  window: DEFAULT_WINDOW,
  anchor: null,
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
