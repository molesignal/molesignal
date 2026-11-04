import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import * as React from 'react';

import type { Span, TraceResponse } from '@/api/web';
import { TraceView } from '@/viz/trace/view/TraceView';

/**
 * Demo route for trace render perf testing
 * (web-playwright-runtime).
 *
 * URL: `/_demo/trace?spans=100000`
 *
 * Synthesises `N` spans laid out as a balanced binary tree and seeds the
 * `useTrace` react-query cache for `traceId="demo"`, so the trace view
 * renders its virtualised waterfall without touching the network. Only
 * mounted in dev builds.
 */
export function TraceDemo() {
  const params = new URLSearchParams(globalThis.location?.search ?? '');
  const n = clamp(parseInt(params.get('spans') ?? '100000', 10) || 100000, 1, 1_000_000);

  const client = React.useMemo(() => {
    const c = new QueryClient({ defaultOptions: { queries: { staleTime: Infinity, retry: false } } });
    c.setQueryData(['web', 'trace', 'demo', null, null], buildTrace(n));
    return c;
  }, [n]);

  return (
    <QueryClientProvider client={client}>
      <div className="flex h-screen flex-col">
        <header className="border-b border-border px-3 py-1 font-sans text-type-micro text-muted-foreground">
          /_demo/trace · {n.toLocaleString()} spans
        </header>
        <div className="min-h-0 flex-1">
          <TraceView traceId="demo" frame="page" />
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
      spanId: `s${i}`,
      ...(parentIdx >= 0 && { parentSpanId: `s${parentIdx}` }),
      service: SERVICES[i % SERVICES.length]!,
      operation: OPS[i % OPS.length]!,
      startNs: start,
      endNs: start + dur,
      durationNs: dur,
      kind: 1,
      status: i % 97 === 0 ? 'ERROR' : 'OK',
      traceFlags: 0,
      traceState: '',
      resource: { attributes: {}, dropped_attributes_count: 0 },
      scope: { name: '', version: '', attributes: {}, dropped_attributes_count: 0 },
      attributes: {},
      events: [],
      links: [],
      droppedAttributesCount: 0,
      droppedEventsCount: 0,
      droppedLinksCount: 0,
      schemaVersion: 1,
      semanticConventionsVersion: '1.43.0',
      samplingReason: 'default_ratio',
      partial: false,
      partialReasons: [],
      late: false,
      duplicate: false,
      conflict: false,
    };
  }
  return {
    traceId: 'demo',
    rootSpanId: 's0',
    spans,
    truncated: false,
    partial: false,
    partialReasons: [],
    samplingReasons: ['default_ratio'],
    lateSpanCount: 0,
    duplicateSpanCount: 0,
    conflictSpanCount: 0,
  };
}

function clamp(v: number, lo: number, hi: number): number {
  return Math.max(lo, Math.min(hi, v));
}
