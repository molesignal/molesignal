import { describe, expect, it } from 'vitest';

import {
  computeForceLayout,
  computeLayeredLayout,
  hashGraph,
  useTopologyLayoutCache,
} from '@/viz/topology/forceLayout';

describe('topology layout cache', () => {
  it('hashGraph is stable under id permutation', () => {
    const nodes = [
      { id: 'b', name: 'b', error_rate: 0, p95_ms: 0, rps: 0, span_count: 0 },
      { id: 'a', name: 'a', error_rate: 0, p95_ms: 0, rps: 0, span_count: 0 },
    ];
    const edges = [{ source: 'b', target: 'a', rps: 0, err_rate: 0, p95_ms: 0 }];
    const h1 = hashGraph(nodes, edges, 1000);
    const h2 = hashGraph([...nodes].reverse(), edges, 1000);
    expect(h1).toBe(h2);
  });

  it('keeps layouts for short and tall viewports separate', () => {
    const nodes = [
      { id: 'api', name: 'api', error_rate: 0, p95_ms: 0, rps: 0, span_count: 0 },
    ];
    expect(hashGraph(nodes, [], 1000, 360)).not.toBe(hashGraph(nodes, [], 1000, 720));
  });

  it('keeps densely connected service nodes from overlapping', () => {
    const nodes = ['gateway', 'checkout', 'payment', 'inventory', 'auth', 'bank'].map((id) => ({
      id,
      name: id,
      error_rate: 0,
      p95_ms: 0,
      rps: 0,
      span_count: 0,
    }));
    const edges = [
      ['gateway', 'checkout'],
      ['checkout', 'payment'],
      ['checkout', 'inventory'],
      ['checkout', 'auth'],
      ['payment', 'bank'],
    ].map(([source, target]) => ({
      source: source!,
      target: target!,
      rps: 1,
      err_rate: 0,
      p95_ms: 1,
    }));

    const positions = computeForceLayout(nodes, edges, 1200, 380);
    const pairDistances = nodes.flatMap((node, index) =>
      nodes.slice(index + 1).map((other) => {
        const first = positions[node.id]!;
        const second = positions[other.id]!;
        return Math.hypot(first.x - second.x, first.y - second.y);
      }),
    );

    expect(Math.min(...pairDistances)).toBeGreaterThanOrEqual(150);
  });

  it('lays a service-call chain from left to right in tree mode', () => {
    const nodes = ['web', 'api', 'db'].map((id) => ({
      id,
      name: id,
      error_rate: 0,
      p95_ms: 0,
      rps: 0,
      span_count: 0,
    }));
    const edges = [
      { source: 'web', target: 'api', rps: 1, err_rate: 0, p95_ms: 1 },
      { source: 'api', target: 'db', rps: 1, err_rate: 0, p95_ms: 1 },
    ];

    const positions = computeLayeredLayout(nodes, edges, 1200, 600, 'horizontal');

    expect(positions.web!.x).toBeLessThan(positions.api!.x);
    expect(positions.api!.x).toBeLessThan(positions.db!.x);
  });

  it('lays a service-call chain from top to bottom in vertical mode', () => {
    const nodes = ['web', 'api', 'db'].map((id) => ({
      id,
      name: id,
      error_rate: 0,
      p95_ms: 0,
      rps: 0,
      span_count: 0,
    }));
    const edges = [
      { source: 'web', target: 'api', rps: 1, err_rate: 0, p95_ms: 1 },
      { source: 'api', target: 'db', rps: 1, err_rate: 0, p95_ms: 1 },
    ];

    const positions = computeLayeredLayout(nodes, edges, 1200, 600, 'vertical');

    expect(positions.web!.y).toBeLessThan(positions.api!.y);
    expect(positions.api!.y).toBeLessThan(positions.db!.y);
  });

  it('assigns finite positions to cyclic and disconnected telemetry', () => {
    const nodes = ['a', 'b', 'standalone'].map((id) => ({
      id,
      name: id,
      error_rate: 0,
      p95_ms: 0,
      rps: 0,
      span_count: 0,
    }));
    const edges = [
      { source: 'a', target: 'b', rps: 1, err_rate: 0, p95_ms: 1 },
      { source: 'b', target: 'a', rps: 1, err_rate: 0, p95_ms: 1 },
    ];

    const positions = computeLayeredLayout(nodes, edges, 800, 480);

    expect(Object.keys(positions).sort()).toEqual(['a', 'b', 'standalone']);
    expect(Object.values(positions).every(({ x, y }) => Number.isFinite(x) && Number.isFinite(y))).toBe(true);
  });

  it('cache set + get round-trips by hash', () => {
    const key = 'k1';
    const positions = { web: { x: 0, y: 0 }, api: { x: 100, y: 50 } };
    useTopologyLayoutCache.getState().set(key, positions);
    const got = useTopologyLayoutCache.getState().get(key);
    expect(got?.web).toEqual({ x: 0, y: 0 });
  });

  it('returns undefined for unknown key', () => {
    expect(useTopologyLayoutCache.getState().get('missing-key-xyz')).toBeUndefined();
  });
});
