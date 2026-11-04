import { useQuery } from '@tanstack/react-query';

import * as profilesApi from '@/api/profiles';
import type { ProfileEntry } from '@/api/profiles';
import type { TraceWindow } from '@/api/web';

/** How far around a trace its profiles are looked for: a profile is stamped when its capture began, before the samples it holds. */
const PADDING_MICROS = 5 * 60 * 1_000_000;
/** The most profiles one trace's lookup reads. */
const LIMIT = 1_000;

/** The profiles that kept a trace's id, by the span each kept. */
export interface TraceProfiles {
  bySpan: ReadonlyMap<string, readonly ProfileEntry[]>;
  all: readonly ProfileEntry[];
  /** Where they were looked for, in epoch microseconds. */
  window: TraceWindow;
}

/** Where the profiles of a trace, or of a span of it, are looked for. */
export function profileWindow(startNs: number, endNs: number): TraceWindow {
  return { from: Math.floor(startNs / 1_000) - PADDING_MICROS, to: Math.ceil(endNs / 1_000) + PADDING_MICROS };
}

/**
 * The profiles whose samples kept the trace's id, as OTLP profiles keep the
 * trace and span a sample was taken in. One read for the whole trace: which
 * spans have samples is known before any is opened. A reader who may not
 * read profiles, or a server without them, finds none.
 */
export function useTraceProfiles(traceId: string, startNs: number, endNs: number) {
  const window = profileWindow(startNs, endNs);

  return useQuery({
    queryKey: ['trace', 'profiles', traceId, window.from, window.to],
    queryFn: async (): Promise<TraceProfiles> => {
      const all = await profilesApi.list({ traceId: traceId, from: window.from, to: window.to, limit: LIMIT });
      const bySpan = new Map<string, ProfileEntry[]>();

      for (const profile of all) {
        if (profile.traceId && profile.traceId !== traceId) continue;
        if (profile.spanId) bySpan.set(profile.spanId, [...(bySpan.get(profile.spanId) ?? []), profile]);
      }

      return { bySpan, all, window };
    },
    staleTime: 60_000,
    retry: false,
  });
}

function iso(micros: number): string {
  return new Date(Math.floor(micros / 1_000)).toISOString();
}

/** The Profiles page over a window: with a trace, and a span of it, the profiles that kept them. */
export function profilesHref(window: TraceWindow, link: { traceId?: string | undefined; spanId?: string | undefined } = {}): string {
  const params = new URLSearchParams();
  if (link.traceId) params.set('traceId', link.traceId);
  if (link.spanId) params.set('spanId', link.spanId);
  const from = iso(window.from);
  const to = iso(window.to);
  params.set('from', from);
  params.set('to', to);
  params.set('time', `${from}..${to}`);
  return `/profiles?${params}`;
}
