import type { TraceWindow } from '@/api/web';

/** A trace to open on its own page: which, when its spans began where that is known, and the span to begin on. */
export interface TracePageTarget {
  traceId: string;
  window?: TraceWindow | undefined;
  spanId?: string | null | undefined;
}

/** A minute either side of a trace: every span of it began in that stretch. */
const AROUND_MICROS = 60 * 1_000_000;

/** Where a trace shown in full is looked for again: from a minute before it began to a minute after it ended. */
export function traceWindowAround(startNs: number, endNs: number): TraceWindow | undefined {
  if (!Number.isFinite(startNs) || startNs <= 0 || !Number.isFinite(endNs)) return undefined;
  return { from: Math.floor(startNs / 1_000) - AROUND_MICROS, to: Math.ceil(endNs / 1_000) + AROUND_MICROS };
}

/** The trace's own page, at the span asked for, over the window it was found in. */
export function tracePageHref(trace: TracePageTarget): string {
  const params = new URLSearchParams();
  if (trace.spanId) params.set('spanId', trace.spanId);
  if (trace.window) {
    params.set('from', String(Math.floor(trace.window.from)));
    params.set('to', String(Math.ceil(trace.window.to)));
  }
  return `/traces/${encodeURIComponent(trace.traceId)}${params.size > 0 ? `?${params}` : ''}`;
}
