import type { ServiceHealth } from '@/api/apm';
import type { MapEdge, MapNode, MapRed } from '@/api/apmMap';

/** Past this many nodes a map starts with each namespace folded into one node. */
export const FOLD_FROM_NODES = 60;

/** A namespace folded into one node: its services' traffic together, its worst health. */
export interface GroupNode {
  id: string;
  namespace: string;
  services: number;
  red: MapRed;
  health: ServiceHealth;
}

export type DrawnNode = { node: MapNode; group?: undefined } | { node?: undefined; group: GroupNode };

/** A call as drawn, its id the calls it stands for. */
export interface DrawnEdge extends MapEdge {
  /** The calls a folded namespace merged into this one. */
  merged: string[];
}

const HEALTH_ORDER: readonly ServiceHealth[] = ['no_traffic', 'healthy', 'warning', 'critical'];

export function groupId(namespace: string): string {
  return `group:${namespace}`;
}

/**
 * The map with the services of every namespace in `folded` drawn as one node
 * per namespace; calls between two such nodes or between a node and the rest
 * are merged, their counts summed, their P95 the slowest.
 */
export function foldNamespaces(nodes: readonly MapNode[], edges: readonly MapEdge[], folded: ReadonlySet<string>): { nodes: DrawnNode[]; edges: DrawnEdge[] } {
  const owner = new Map<string, string>();
  const groups = new Map<string, GroupNode>();
  const drawn: DrawnNode[] = [];

  for (const node of nodes) {
    const namespace = node.kind === 'service' ? node.namespace : undefined;

    if (!namespace || !folded.has(namespace)) {
      drawn.push({ node });
      continue;
    }

    const id = groupId(namespace);
    const group = groups.get(id) ?? { id, namespace, services: 0, red: { requestCount: 0, errorCount: 0, errorRate: 0, p95Time: null }, health: 'no_traffic' };
    group.services += 1;
    group.red = addRed(group.red, node.red);
    group.health = worse(group.health, node.health ?? 'no_traffic');
    groups.set(id, group);
    owner.set(node.id, id);
  }

  drawn.push(...[...groups.values()].map((group) => ({ group })));

  const merged = new Map<string, DrawnEdge>();

  for (const edge of edges) {
    const source = owner.get(edge.source) ?? edge.source;
    const target = owner.get(edge.target) ?? edge.target;

    if (source === target) continue;

    const id = `${source}->${target}`;
    const current = merged.get(id);

    if (!current) {
      merged.set(id, { ...edge, id, source, target, merged: [edge.id] });
      continue;
    }

    current.red = addRed(current.red, edge.red);
    current.merged.push(edge.id);
    // A merged call is new or gone only when every call in it is.
    if (current.change !== edge.change) delete current.change;
  }

  return { nodes: drawn, edges: [...merged.values()] };
}

/** Every node `id` is reached from (`up`) or reaches (`down`), along the calls. */
export function reach(edges: readonly { source: string; target: string }[], id: string, direction: 'up' | 'down'): Set<string> {
  const next = new Map<string, string[]>();

  for (const edge of edges) {
    const [from, to] = direction === 'down' ? [edge.source, edge.target] : [edge.target, edge.source];
    next.set(from, [...(next.get(from) ?? []), to]);
  }

  const seen = new Set<string>();
  const queue = [...(next.get(id) ?? [])];

  while (queue.length > 0) {
    const current = queue.shift()!;
    if (seen.has(current) || current === id) continue;
    seen.add(current);
    queue.push(...(next.get(current) ?? []));
  }

  return seen;
}

/**
 * The slowest way down from `id`: at each node the call with the highest
 * P95, until a node calls nothing it has not been through. The calls' ids.
 */
export function criticalPath(edges: readonly DrawnEdge[], id: string): string[] {
  const path: string[] = [];
  const visited = new Set([id]);
  let current = id;

  for (;;) {
    const slowest = edges
      .filter((edge) => edge.source === current && !visited.has(edge.target) && edge.change !== 'gone')
      .sort((left, right) => (right.red.p95Time ?? 0) - (left.red.p95Time ?? 0))[0];

    if (!slowest) return path;

    path.push(slowest.id);
    visited.add(slowest.target);
    current = slowest.target;
  }
}

/** Requests a second over `seconds`. */
export function perSecond(red: MapRed, seconds: number): number {
  return seconds > 0 ? red.requestCount / seconds : 0;
}

function addRed(left: MapRed, right: MapRed): MapRed {
  const request_count = left.requestCount + right.requestCount;
  const error_count = left.errorCount + right.errorCount;
  return {
    requestCount: request_count,
    errorCount: error_count,
    errorRate: request_count > 0 ? error_count / request_count : 0,
    p95Time: Math.max(left.p95Time ?? 0, right.p95Time ?? 0) || null,
  };
}

function worse(left: ServiceHealth, right: ServiceHealth): ServiceHealth {
  return HEALTH_ORDER.indexOf(right) > HEALTH_ORDER.indexOf(left) ? right : left;
}
