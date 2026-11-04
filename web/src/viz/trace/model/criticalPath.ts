import type { TraceNode, TraceTree } from './tree';

/** A stretch of the critical path: the span whose own work held the trace up from `fromNs` to `toNs`, as epoch nanoseconds. */
export interface CriticalSegment {
  id: string;
  fromNs: number;
  toNs: number;
}

/** A service's part of the critical path. */
export interface CriticalService {
  name: string;
  color: string;
  ns: number;
  /** That time as a part of the path, from 0 to 1. */
  share: number;
}

export interface CriticalPath {
  /** The stretches of the path, the earliest first. */
  segments: CriticalSegment[];
  /** Each span's stretches on the path, the earliest first. */
  segmentsById: ReadonlyMap<string, readonly CriticalSegment[]>;
  /** Every span the path runs through: those it spends time in and those above them. */
  spans: ReadonlySet<string>;
  /** Each span's time on the path. */
  nsById: ReadonlyMap<string, number>;
  /** The services by their time on the path, the most first; a service with none is left out. */
  services: CriticalService[];
  /** The length of the path: as long as the root span ran. */
  totalNs: number;
}

/** A span as the walk sees it: cut to the stretch of its caller it ran within. */
interface Clipped {
  node: TraceNode;
  fromNs: number;
  toNs: number;
}

/** A span the walk is in: how far back from its end the walk has got, and which of its children are spent. */
interface Frame extends Clipped {
  cursor: number;
  /** Its children by when they end, the last first. */
  children: Clipped[];
  next: number;
}

/**
 * The spans that the trace's end-to-end time waited on. The walk starts at
 * the root's end and goes back in time: at each moment it follows the child
 * that ended last by then, down into that child, and on from the moment the
 * child began. A child still running at that moment ran alongside the one
 * the walk followed, and held nothing up: a call made without waiting for
 * it, or one of several made at once, is not on the path. Time no child
 * covers is the span's own. A child is cut to its caller's span: what it did
 * before the caller began or after the caller returned is no part of the
 * caller's time.
 */
export function criticalPath(tree: TraceTree): CriticalPath {
  const segments: CriticalSegment[] = [];
  const spans = new Set<string>();
  const nsById = new Map<string, number>();
  const { root } = tree;

  const add = (node: TraceNode, fromNs: number, toNs: number) => {
    if (toNs <= fromNs) return;
    segments.push({ id: node.id, fromNs, toNs });
    nsById.set(node.id, (nsById.get(node.id) ?? 0) + toNs - fromNs);
  };

  // No recursion: a chain of calls can be deeper than the call stack.
  const stack: Frame[] = [frameOf({ node: root, fromNs: root.startNs, toNs: root.endNs })];

  while (stack.length > 0) {
    const top = stack[stack.length - 1]!;
    spans.add(top.node.id);
    const child = lastEnded(top);

    if (!child) {
      add(top.node, top.fromNs, top.cursor);
      stack.pop();
      continue;
    }

    add(top.node, child.toNs, top.cursor);
    // Back in this span, the walk goes on from the moment the child began.
    top.cursor = child.fromNs;
    stack.push(frameOf(child));
  }

  segments.sort((left, right) => left.fromNs - right.fromNs);

  const segmentsById = new Map<string, CriticalSegment[]>();
  for (const segment of segments) segmentsById.set(segment.id, [...(segmentsById.get(segment.id) ?? []), segment]);

  const totalNs = segments.reduce((total, segment) => total + segment.toNs - segment.fromNs, 0);

  return { segments, segmentsById, spans, nsById, services: servicesOf(tree, nsById, totalNs), totalNs };
}

function frameOf(span: Clipped): Frame {
  const children: Clipped[] = [];

  for (const child of span.node.children) {
    const fromNs = Math.max(child.startNs, span.fromNs);
    const toNs = Math.min(child.endNs, span.toNs);
    if (toNs > fromNs) children.push({ node: child, fromNs, toNs });
  }

  // The last to end first; of two that end together, the longer.
  children.sort((left, right) => right.toNs - left.toNs || left.fromNs - right.fromNs);

  return { ...span, cursor: span.toNs, children, next: 0 };
}

/**
 * The child that ended last by the frame's cursor; of those that ended
 * together, the longer. A child that ends past the cursor ran alongside the
 * child the walk followed before: it is passed over, and can never be on the
 * path later, since the cursor only moves back.
 */
function lastEnded(frame: Frame): Clipped | null {
  const { children, cursor } = frame;

  while (frame.next < children.length && children[frame.next]!.toNs > cursor) frame.next += 1;
  if (frame.next >= children.length) return null;

  // Sorted by end, the longest first among equal ends: the first one left is the one.
  const child = children[frame.next]!;
  frame.next += 1;
  return child;
}

function servicesOf(tree: TraceTree, nsById: ReadonlyMap<string, number>, totalNs: number): CriticalService[] {
  const byService = new Map<string, number>();

  for (const [id, ns] of nsById) {
    const service = tree.byId.get(id)?.service;
    if (service !== undefined) byService.set(service, (byService.get(service) ?? 0) + ns);
  }

  return tree.services
    .filter((service) => (byService.get(service.name) ?? 0) > 0)
    .map((service) => {
      const ns = byService.get(service.name) ?? 0;
      return { name: service.name, color: service.color, ns, share: totalNs > 0 ? ns / totalNs : 0 };
    })
    .sort((left, right) => right.ns - left.ns || left.name.localeCompare(right.name));
}
