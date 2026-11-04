import { describe, expect, it } from 'vitest';

import type { MapEdge, MapNode } from '@/api/apmMap';

import { criticalPath, type DrawnEdge, foldNamespaces, reach } from './graph';

function service(name: string, namespace: string, requests = 10, health: MapNode['health'] = 'healthy'): MapNode {
  return { id: `service:${name}`, kind: 'service', name, namespace, red: { requestCount: requests, errorCount: 0, errorRate: 0, p95Time: 1_000 }, health };
}

function edge(source: string, target: string, p95 = 1_000, requests = 10): MapEdge {
  return { id: `${source}->${target}`, source, target, kind: 'call', red: { requestCount: requests, errorCount: 1, errorRate: 1 / requests, p95Time: p95 } };
}

const drawn = (edges: MapEdge[]): DrawnEdge[] => edges.map((item) => ({ ...item, merged: [item.id] }));

describe('the APM map graph', () => {
  it('folds a namespace into one node and merges its calls', () => {
    const nodes = [service('web', 'edge'), service('checkout', 'shop', 20, 'warning'), service('cart', 'shop', 5, 'critical'), { id: 'database:pg', kind: 'external' as const, name: 'pg', red: { requestCount: 3, errorCount: 0, errorRate: 0 } }];
    const edges = [edge('service:web', 'service:checkout', 2_000), edge('service:web', 'service:cart', 5_000), edge('service:checkout', 'service:cart'), edge('service:cart', 'database:pg')];

    const folded = foldNamespaces(nodes, edges, new Set(['shop']));
    const group = folded.nodes.find((entry) => entry.group)?.group;

    expect(group).toMatchObject({ id: 'group:shop', services: 2, health: 'critical' });
    expect(group?.red.requestCount).toBe(25);
    expect(folded.edges.map((item) => item.id).sort()).toEqual(['group:shop->database:pg', 'service:web->group:shop']);
    expect(folded.edges.find((item) => item.id === 'service:web->group:shop')).toMatchObject({ red: { requestCount: 20, p95Time: 5_000 }, merged: ['service:web->service:checkout', 'service:web->service:cart'] });
  });

  it('reaches up and down the calls, through cycles', () => {
    const edges = [edge('a', 'b'), edge('b', 'c'), edge('c', 'b'), edge('x', 'a')];

    expect([...reach(edges, 'b', 'down')].sort()).toEqual(['c']);
    expect([...reach(edges, 'b', 'up')].sort()).toEqual(['a', 'c', 'x']);
  });

  it('follows the slowest call down, never back', () => {
    const edges = drawn([edge('a', 'b', 100), edge('a', 'c', 900), edge('c', 'd', 50), edge('c', 'a', 10_000), edge('d', 'e', 70)]);

    expect(criticalPath(edges, 'a')).toEqual(['a->c', 'c->d', 'd->e']);
    expect(criticalPath(edges, 'e')).toEqual([]);
  });
});
