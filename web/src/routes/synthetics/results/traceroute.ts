/** How far either side of a step its trace is looked for: an hour, as a linked span is. */
const TRACE_SEARCH_PADDING_MICROS = 60 * 60 * 1_000_000;

/** One hop of a traceroute, as the Agent reports it. */
export interface TracerouteHop {
  ttl: number;
  /** The address that answered most often, or null when none did. */
  address: string | null;
  /** How many addresses answered at this distance: more than one is a route that branches. */
  addresses: number;
  sent: number;
  received: number;
  bestMs: number | null;
  averageMs: number | null;
  worstMs: number | null;
}

/**
 * The hops of a traceroute step, from its `hops` metadata: an array of
 * `[ttl, address, addresses, sent, received, best, average, worst]`.
 */
export function tracerouteHops(hops: string | undefined): TracerouteHop[] {
  if (!hops) return [];

  let parsed: unknown;
  try {
    parsed = JSON.parse(hops);
  } catch {
    return [];
  }

  if (!Array.isArray(parsed)) return [];

  const count = (value: unknown) => (typeof value === 'number' ? value : 0);
  const millis = (value: unknown) => (typeof value === 'number' ? value : null);

  return parsed.flatMap((hop: unknown) =>
    Array.isArray(hop) && typeof hop[0] === 'number'
      ? [
          {
            ttl: hop[0],
            address: typeof hop[1] === 'string' ? hop[1] : null,
            addresses: count(hop[2]),
            sent: count(hop[3]),
            received: count(hop[4]),
            bestMs: millis(hop[5]),
            averageMs: millis(hop[6]),
            worstMs: millis(hop[7]),
          },
        ]
      : [],
  );
}

/** The share of the probes to a hop that got no answer. */
export function lossRatio(hop: TracerouteHop): number {
  return hop.sent > 0 ? (hop.sent - hop.received) / hop.sent : 0;
}

/**
 * The trace a step's requests carried, looked for around the time of the step: the services it
 * reached keep their spans under it, when they are traced at all.
 */
export function traceLink(traceId: string, startedAt: number, finishedAt: number): string {
  const params = new URLSearchParams({
    from: String(startedAt - TRACE_SEARCH_PADDING_MICROS),
    to: String(finishedAt + TRACE_SEARCH_PADDING_MICROS),
  });

  return `/traces/${encodeURIComponent(traceId)}?${params}`;
}
