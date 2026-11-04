import * as React from 'react';

import { useUiPreference } from '@/shell/uiPreference/useUiPreference';

/** The widths of a trace's view a reader set, in CSS pixels. */
export interface TraceLayoutWidths {
  /** The drawer the trace opens in over the list. */
  drawer?: number;
  /** The detail of a span beside the waterfall. */
  panel?: number;
  /** The column of span names in the waterfall. */
  names?: number;
}

export const TRACE_LAYOUT_LIMITS = {
  drawer: { min: 720, fallbackShare: 0.8 },
  panel: { min: 320, max: 720, fallback: 420 },
  names: { min: 200, fallback: 340 },
} as const;

const KEYS = ['drawer', 'panel', 'names'] as const;

function parseLayout(raw: unknown): TraceLayoutWidths | null {
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) return null;

  const layout: TraceLayoutWidths = {};
  for (const key of KEYS) {
    const value = (raw as Record<string, unknown>)[key];
    if (typeof value === 'number' && Number.isFinite(value) && value >= 160 && value <= 4_000) layout[key] = Math.round(value);
  }

  return layout;
}

/**
 * The widths of a trace's view, as the reader left them. A drag shows at once
 * and is kept when it is let go, as a preference of the signed-in user.
 */
export function useTraceLayout(): {
  widths: TraceLayoutWidths;
  /** Shows a width while it is being set. */
  preview: (part: TraceLayoutWidths) => void;
  /** Keeps a width. */
  commit: (part: TraceLayoutWidths) => void;
} {
  const kept = useUiPreference('traces.layout', 'view', parseLayout);
  const [live, setLive] = React.useState<TraceLayoutWidths>({});
  const widths = React.useMemo(() => ({ ...(kept.value ?? {}), ...live }), [kept.value, live]);

  const keptRef = React.useRef(kept);
  keptRef.current = kept;
  const liveRef = React.useRef(live);
  liveRef.current = live;

  const preview = React.useCallback((part: TraceLayoutWidths) => setLive((current) => ({ ...current, ...part })), []);

  const commit = React.useCallback((part: TraceLayoutWidths) => {
    const next = { ...(keptRef.current.value ?? {}), ...liveRef.current, ...part };
    setLive(next);
    keptRef.current.save(next);
  }, []);

  return { widths, preview, commit };
}
