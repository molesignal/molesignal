import {
  forceCenter,
  forceCollide,
  forceLink,
  forceManyBody,
  forceSimulation,
  type Simulation,
} from 'd3-force';
import { create } from 'zustand';

import type { TopologyEdge, TopologyNode } from '@/api/web';

interface LayoutCache {
  byHash: Record<string, Record<string, { x: number; y: number }>>;
  get: (hash: string) => Record<string, { x: number; y: number }> | undefined;
  set: (hash: string, positions: Record<string, { x: number; y: number }>) => void;
}

export const useTopologyLayoutCache = create<LayoutCache>((set, get) => ({
  byHash: {},
  get: (hash) => get().byHash[hash],
  set: (hash, positions) => set({ byHash: { ...get().byHash, [hash]: positions } }),
}));

export function hashGraph(
  nodes: TopologyNode[],
  edges: TopologyEdge[],
  viewportWidth: number,
  viewportHeight = 0,
): string {
  // Cheap stable hash: sort ids + viewport buckets. Height matters because the
  // service graph is also embedded in short panels where a portrait layout
  // would otherwise be reused from a full-page canvas.
  const nodeIds = nodes
    .map((n) => n.id)
    .sort()
    .join(',');
  const edgeIds = edges
    .map((e) => `${e.source}>${e.target}`)
    .sort()
    .join(',');
  const widthBucket = Math.round(viewportWidth / 200);
  const heightBucket = Math.round(viewportHeight / 120);
  return `${nodeIds}|${edgeIds}|${widthBucket}x${heightBucket}`;
}

interface D3Node {
  id: string;
  x?: number;
  y?: number;
  vx?: number;
  vy?: number;
}

export type TopologyDirection = 'horizontal' | 'vertical';

/**
 * Deterministic directed layout for service-call trees. It intentionally does
 * not assume the graph is a perfect DAG: cyclic and disconnected components
 * are assigned a stable layer as well, so malformed telemetry can still be
 * inspected instead of collapsing every service onto the same point.
 */
export function computeLayeredLayout(
  nodes: TopologyNode[],
  edges: TopologyEdge[],
  width: number,
  height: number,
  direction: TopologyDirection = 'horizontal',
): Record<string, { x: number; y: number }> {
  const ids = nodes.map((node) => node.id).sort();
  const validIds = new Set(ids);
  const outgoing = new Map(ids.map((id) => [id, [] as string[]]));
  const incoming = new Map(ids.map((id) => [id, [] as string[]]));

  for (const edge of edges) {
    if (!validIds.has(edge.source) || !validIds.has(edge.target) || edge.source === edge.target) {
      continue;
    }
    outgoing.get(edge.source)!.push(edge.target);
    incoming.get(edge.target)!.push(edge.source);
  }
  for (const adjacent of [...outgoing.values(), ...incoming.values()]) adjacent.sort();

  const remainingIncoming = new Map(ids.map((id) => [id, incoming.get(id)!.length]));
  const depth = new Map<string, number>();
  const processed = new Set<string>();
  const queue = ids.filter((id) => remainingIncoming.get(id) === 0);
  for (const root of queue) depth.set(root, 0);

  for (let index = 0; index < queue.length; index += 1) {
    const source = queue[index]!;
    processed.add(source);
    for (const target of outgoing.get(source)!) {
      depth.set(target, Math.max(depth.get(target) ?? 0, (depth.get(source) ?? 0) + 1));
      const nextIncoming = (remainingIncoming.get(target) ?? 1) - 1;
      remainingIncoming.set(target, nextIncoming);
      if (nextIncoming === 0) queue.push(target);
    }
  }

  // Kahn's pass above intentionally leaves cycles behind. Walk each remaining
  // component once, preserving any depth inherited from an acyclic parent.
  for (const seed of ids) {
    if (processed.has(seed)) continue;
    const componentQueue = [seed];
    if (!depth.has(seed)) depth.set(seed, 0);
    processed.add(seed);
    for (let index = 0; index < componentQueue.length; index += 1) {
      const source = componentQueue[index]!;
      for (const target of outgoing.get(source)!) {
        if (processed.has(target)) continue;
        depth.set(target, (depth.get(source) ?? 0) + 1);
        processed.add(target);
        componentQueue.push(target);
      }
    }
  }

  const layers = new Map<number, string[]>();
  for (const id of ids) {
    const layer = depth.get(id) ?? 0;
    const members = layers.get(layer) ?? [];
    members.push(id);
    layers.set(layer, members);
  }

  // A light barycentric sweep keeps converging calls near their parents and
  // removes most avoidable crossings without introducing another dependency.
  const orderedDepths = [...layers.keys()].sort((a, b) => a - b);
  for (const layer of orderedDepths) layers.get(layer)!.sort();
  for (const layer of orderedDepths.slice(1)) {
    const previous = layers.get(layer - 1) ?? [];
    const previousOrder = new Map(previous.map((id, index) => [id, index]));
    layers.get(layer)!.sort((left, right) => {
      const score = (id: string) => {
        const parentIndexes = incoming
          .get(id)!
          .map((parent) => previousOrder.get(parent))
          .filter((value): value is number => value !== undefined);
        return parentIndexes.length === 0
          ? Number.POSITIVE_INFINITY
          : parentIndexes.reduce((sum, value) => sum + value, 0) / parentIndexes.length;
      };
      return score(left) - score(right) || left.localeCompare(right);
    });
  }

  const maxLayerSize = Math.max(1, ...[...layers.values()].map((members) => members.length));
  const crossViewport = direction === 'horizontal' ? Math.max(height, 360) : Math.max(width, 640);
  const crossGap = direction === 'horizontal' ? 72 : 220;
  const nodeCrossSize = direction === 'horizontal' ? 44 : 200;
  const requiredCrossSize = (maxLayerSize - 1) * crossGap + nodeCrossSize + 96;
  const crossSize = Math.max(crossViewport, requiredCrossSize);
  const mainGap = direction === 'horizontal' ? 260 : 132;
  const mainStart = 48;
  const positions: Record<string, { x: number; y: number }> = {};

  for (const layer of orderedDepths) {
    const members = layers.get(layer)!;
    const occupied = (members.length - 1) * crossGap + nodeCrossSize;
    const crossStart = Math.max(32, (crossSize - occupied) / 2);
    members.forEach((id, index) => {
      const main = mainStart + layer * mainGap;
      const cross = crossStart + index * crossGap;
      positions[id] = direction === 'horizontal'
        ? { x: main, y: cross }
        : { x: cross, y: main };
    });
  }

  return positions;
}

/**
 * One-shot d3-force layout. Stops after `ticks` and returns a {nodeId: {x, y}}
 * map suitable for ReactFlow positions.
 */
export function computeForceLayout(
  nodes: TopologyNode[],
  edges: TopologyEdge[],
  width: number,
  height: number,
  ticks = 300,
): Record<string, { x: number; y: number }> {
  const d3Nodes: D3Node[] = nodes.map((n) => ({ id: n.id }));
  const d3Edges = edges.map((e) => ({ source: e.source, target: e.target }));
  const availableWidth = Math.max(width, 640);
  const availableHeight = Math.max(height, 360);
  const areaPerNode = Math.sqrt(
    (availableWidth * availableHeight) / Math.max(d3Nodes.length, 1),
  );
  const linkDistance = Math.max(180, Math.min(240, areaPerNode * 0.72));
  // ServiceNode can be up to 120px wide once its label is included. A collision
  // radius of 84px keeps both circles and labels apart at every zoom level.
  const collisionRadius = 84;

  const sim: Simulation<D3Node, undefined> = forceSimulation<D3Node>(d3Nodes)
    .force(
      'link',
      forceLink<D3Node, { source: string; target: string }>(d3Edges)
        .id((d) => d.id)
        .distance(linkDistance)
        .strength(0.55),
    )
    .force('charge', forceManyBody().strength(-700))
    .force(
      'collision',
      forceCollide<D3Node>(collisionRadius)
        .strength(1)
        .iterations(4),
    )
    .force('center', forceCenter(width / 2, height / 2))
    .stop();

  for (let i = 0; i < ticks; i++) sim.tick();

  const out: Record<string, { x: number; y: number }> = {};
  for (const n of d3Nodes) {
    out[n.id] = { x: n.x ?? 0, y: n.y ?? 0 };
  }
  return out;
}
