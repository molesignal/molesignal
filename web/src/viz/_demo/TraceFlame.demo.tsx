import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import * as React from 'react';

import type { Span, TraceResponse } from '@/api/web';
import { TraceFlame } from '@/viz/trace/TraceFlame';

/**
 * Demo route for trace-flame perf testing
 * (web-playwright-runtime).
 *
 * URL: `/_demo/trace?spans=100000`
 *
 * Synthesises `N` spans laid out as a balanced binary tree and seeds the
 * `useTrace` react-query cache for `traceId="demo"`, so TraceFlame renders
 * its canvas without touching the network. Only mounted in dev builds.
 */
export function TraceFlameDemo() {
  const params = new URLSearchParams(globalThis.location?.search ?? '');
  const n = clamp(parseInt(params.get('spans') ?? '100000', 10) || 100000, 1, 1_000_000);

  const client = React.useMemo(() => {
    const c = new QueryClient({ defaultOptions: { queries: { staleTime: Infinity, retry: false } } });
    c.setQueryData(['web', 'trace', 'demo'], buildTrace(n));
    return c;
  }, [n]);

  return (
    <QueryClientProvider client={client}>
      <div className="flex h-screen flex-col">
        <header className="border-b border-border px-3 py-1 font-sans text-type-micro text-muted-foreground">
          /_demo/trace · {n.toLocaleString()} spans
        </header>
        <div className="flex-1">
          <TraceFlame traceId="demo" />
        </div>
      </div>
    </QueryClientProvider>
  );
}

function buildTrace(n: number): TraceResponse {
  const spans: Span[] = new Array(n);
  const SERVICES = ['web', 'api', 'auth', 'db', 'cache', 'queue', 'worker'];
  const OPS = ['GET /', 'POST /api', 'SELECT', 'UPDATE', 'INSERT', 'enqueue', 'process'];
  // Total duration ~5s; each span gets a slice proportional to depth.
  const totalNs = 5_000_000_000;
  for (let i = 0; i < n; i++) {
    const parentIdx = i === 0 ? -1 : Math.floor((i - 1) / 2);
    const depth = Math.floor(Math.log2(i + 1));
    const start = i === 0 ? 0 : Math.floor((i / n) * totalNs * 0.9);
    const dur = Math.floor((totalNs / (1 << depth)) * 0.8);
    spans[i] = {
      span_id: `s${i}`,
      ...(parentIdx >= 0 && { parent_span_id: `s${parentIdx}` }),
      service: SERVICES[i % SERVICES.length]!,
      operation: OPS[i % OPS.length]!,
      start_ns: start,
      end_ns: start + dur,
      duration_ns: dur,
      kind: 1,
      status: i % 97 === 0 ? 'ERROR' : 'OK',
      trace_flags: 0,
      trace_state: '',
      resource: { attributes: {}, dropped_attributes_count: 0 },
      scope: { name: '', version: '', attributes: {}, dropped_attributes_count: 0 },
      attributes: {},
      events: [],
      links: [],
      dropped_attributes_count: 0,
      dropped_events_count: 0,
      dropped_links_count: 0,
      schema_version: 1,
      semantic_conventions_version: '1.43.0',
      sampling_reason: 'default_ratio',
      partial: false,
      partial_reasons: [],
      late: false,
      duplicate: false,
      conflict: false,
    };
  }
  return {
    trace_id: 'demo',
    root_span_id: 's0',
    spans,
    truncated: false,
    partial: false,
    partial_reasons: [],
    sampling_reasons: ['default_ratio'],
    late_span_count: 0,
    duplicate_span_count: 0,
    conflict_span_count: 0,
  };
}

function clamp(v: number, lo: number, hi: number): number {
  return Math.max(lo, Math.min(hi, v));
}
