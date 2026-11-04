import type { TraceWindow } from '@/api/web';

/** A trace of a comparison: which, and when its spans began, where that is known. */
export interface ComparedTrace {
  traceId: string;
  window?: TraceWindow | undefined;
}

/** The trace page's parameter naming the trace it is compared with, and the moments that one is looked for in. */
export const COMPARE_PARAMS = { trace: 'compare', from: 'compareFrom', to: 'compareTo' } as const;

/**
 * The first trace's page, comparing it with the second: the second and the
 * windows both are looked for in ride in the address, so the comparison can
 * be shared, and the page goes back to the first trace alone when they go.
 */
export function traceCompareHref(a: ComparedTrace, b: ComparedTrace): string {
  const params = new URLSearchParams();
  params.set(COMPARE_PARAMS.trace, b.traceId);
  if (a.window) {
    params.set('from', String(Math.floor(a.window.from)));
    params.set('to', String(Math.ceil(a.window.to)));
  }
  if (b.window) {
    params.set(COMPARE_PARAMS.from, String(Math.floor(b.window.from)));
    params.set(COMPARE_PARAMS.to, String(Math.ceil(b.window.to)));
  }
  return `/traces/${encodeURIComponent(a.traceId)}?${params}`;
}
