import { describe, expect, it } from 'vitest';

import { lossRatio, traceLink, tracerouteHops } from './traceroute';

describe('traceroute evidence', () => {
  it('reads each hop, answered or silent', () => {
    const hops = tracerouteHops(
      JSON.stringify([
        [1, '192.0.2.1', 1, 4, 4, 0.4, 0.6, 0.9],
        [2, null, 0, 4, 0, null, null, null],
        [3, '198.51.100.7', 2, 4, 3, 10.2, 12.5, 15.1],
      ]),
    );

    expect(hops).toHaveLength(3);
    expect(hops[1]).toEqual({
      ttl: 2,
      address: null,
      addresses: 0,
      sent: 4,
      received: 0,
      bestMs: null,
      averageMs: null,
      worstMs: null,
    });
    expect(hops.map(lossRatio)).toEqual([0, 1, 0.25]);
  });

  it('reads nothing from hops that are not there or not JSON', () => {
    expect(tracerouteHops(undefined)).toEqual([]);
    expect(tracerouteHops('[[1,')).toEqual([]);
    expect(tracerouteHops('{"ttl":1}')).toEqual([]);
  });

  it('looks for a trace around the time of its step', () => {
    const link = traceLink('4bf92f3577b34da6a3ce929d0e0e4736', 5_000_000_000, 5_002_000_000);
    const url = new URL(link, 'https://molesignal.test');

    expect(url.pathname).toBe('/traces/4bf92f3577b34da6a3ce929d0e0e4736');
    expect(Number(url.searchParams.get('from'))).toBeLessThan(5_000_000_000);
    expect(Number(url.searchParams.get('to'))).toBeGreaterThan(5_002_000_000);
  });
});
