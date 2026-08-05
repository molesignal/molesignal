import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import * as React from 'react';

import type { TopologyEdge, TopologyNode, TopologyResponse } from '@/api/web';
import { ServiceTopology } from '@/viz/topology/ServiceTopology';

/**
 * Demo route for topology perf testing
 * (web-playwright-runtime).
 *
 * URL: `/_demo/topology?nodes=200`
 *
 * Builds an `N`-node random graph (~2x edges as nodes) and seeds the
 * `useTopology` react-query cache for the demo time window. ServiceTopology
 * then runs its force layout + ReactFlow render against the synthetic graph.
 */
export function TopologyDemo() {
  const params = new URLSearchParams(globalThis.location?.search ?? '');
  const n = clamp(parseInt(params.get('nodes') ?? '200', 10) || 200, 1, 5_000);

  const FROM = '2026-05-23T09:00:00.000Z';
  const TO = '2026-05-23T10:00:00.000Z';

  const client = React.useMemo(() => {
    const c = new QueryClient({ defaultOptions: { queries: { staleTime: Infinity, retry: false } } });
    c.setQueryData(['web', 'topology', FROM, TO], buildTopology(n));
    return c;
  }, [n]);

  return (
    <QueryClientProvider client={client}>
      <div className="flex h-screen flex-col">
        <header className="border-b border-border px-3 py-1 font-sans text-type-micro text-muted-foreground">
          /_demo/topology · {n.toLocaleString()} nodes
        </header>
        <div className="flex-1">
          <ServiceTopology from={FROM} to={TO} />
        </div>
      </div>
    </QueryClientProvider>
  );
}

function buildTopology(n: number): TopologyResponse {
  const nodes: TopologyNode[] = new Array(n);
  for (let i = 0; i < n; i++) {
    nodes[i] = {
      id: `svc-${i}`,
      name: `svc-${i}`,
      error_rate: (i % 17) / 100,
      p95_ms: 20 + ((i * 7) % 400),
      rps: 5 + ((i * 11) % 200),
      span_count: 100 + i * 13,
    };
  }
  const edges: TopologyEdge[] = [];
  // ~2x edges per node, deterministic so layout is stable across runs.
  for (let i = 0; i < n; i++) {
    const a = `svc-${i}`;
    const b1 = `svc-${(i + 3) % n}`;
    const b2 = `svc-${(i + 7) % n}`;
    if (a !== b1) edges.push({ source: a, target: b1, rps: 10 + i, err_rate: 0.01, p95_ms: 50 });
    if (a !== b2) edges.push({ source: a, target: b2, rps: 5 + i, err_rate: 0.005, p95_ms: 30 });
  }
  return { nodes, edges };
}

function clamp(v: number, lo: number, hi: number): number {
  return Math.max(lo, Math.min(hi, v));
}
