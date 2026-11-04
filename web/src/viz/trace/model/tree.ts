import type { Span, TraceResponse } from '@/api/web';

/** A span in the tree of its trace, with what the views work out for it. */
export interface TraceNode {
  span: Span;
  id: string;
  /** The span it was called from; `null` for a root, and for a span whose parent was never received. */
  parentId: string | null;
  /** It names a parent the trace does not hold: it stands as a root of its own. */
  orphan: boolean;
  depth: number;
  startNs: number;
  endNs: number;
  /** When it began, counted from the start of the trace. */
  offsetNs: number;
  durationNs: number;
  /** Its time spent outside the spans it called: its duration less the time any of them ran. */
  selfNs: number;
  children: TraceNode[];
  /** How many spans lie under it, at any depth. */
  descendants: number;
  service: string;
  failed: boolean;
}

/** A service of the trace, in the order the trace enters it. */
export interface TraceService {
  name: string;
  /** The colour its spans are drawn in, as a CSS value. */
  color: string;
  spans: number;
  errors: number;
  /** The time its spans spent on their own work. */
  selfNs: number;
  /** That time as a part of the work of the whole trace, from 0 to 1. */
  share: number;
}

export interface TraceTree {
  traceId: string;
  trace: TraceResponse;
  /** The spans without a parent in the trace, the trace's own root first, the rest by start. */
  roots: TraceNode[];
  /** Every span, each before the spans it called. */
  nodes: TraceNode[];
  byId: Map<string, TraceNode>;
  /** The trace's own root: the span the request began with. */
  root: TraceNode;
  startNs: number;
  endNs: number;
  /** From the first start to the last end; never 0. */
  totalNs: number;
  services: TraceService[];
  maxDepth: number;
  /** Spans whose parent the trace does not hold. */
  orphanCount: number;
  errorCount: number;
}

/** The categorical colours of the charts. None is red: red is a failed span's. */
const SERVICE_COLORS = 8;

export function serviceColor(index: number): string {
  return `var(--chart-${(index % SERVICE_COLORS) + 1})`;
}

/**
 * The tree of a trace's spans. A span whose parent is missing stands as a
 * root of its own instead of failing the trace: a trace is often received in
 * part. `null` for a trace without spans.
 */
export function buildTraceTree(trace: TraceResponse): TraceTree | null {
  const spans = uniqueSpans(trace.spans);
  if (spans.length === 0) return null;

  const nodes = new Map<string, TraceNode>();
  for (const span of spans) nodes.set(span.spanId, nodeOf(span));

  for (const node of nodes.values()) {
    const parent = node.span.parentSpanId ? nodes.get(node.span.parentSpanId) : undefined;
    if (parent && parent !== node) {
      node.parentId = parent.id;
      parent.children.push(node);
    } else if (node.span.parentSpanId) {
      node.orphan = true;
    }
  }

  for (const node of nodes.values()) node.children.sort(byStart);

  const ordered = [...nodes.values()].sort(byStart);
  const declaredRoot = nodes.get(trace.rootSpanId);
  const rootCandidates = ordered.filter((node) => node.parentId === null);
  const root = (declaredRoot && declaredRoot.parentId === null ? declaredRoot : undefined)
    ?? rootCandidates.find((node) => !node.orphan)
    ?? rootCandidates[0]
    ?? ordered[0]!;

  const roots = [root, ...rootCandidates.filter((node) => node !== root)];
  // Only a loop of parents leaves a root with a parent: the loop is cut above it.
  if (root.parentId !== null) detach(root, nodes);

  const tree: TraceNode[] = [];
  const visited = new Set<string>();

  for (const start of roots) walk(start, 0, tree, visited);

  // A loop of parents leaves spans that no root leads to: each stands as a root.
  for (const node of ordered) {
    if (visited.has(node.id)) continue;
    detach(node, nodes);
    roots.push(node);
    walk(node, 0, tree, visited);
  }

  // Not spread into Math.min: a trace of 100,000 spans is more arguments than a call takes.
  let startNs = Infinity;
  let endNs = -Infinity;
  let maxDepth = 0;

  for (const node of tree) {
    startNs = Math.min(startNs, node.startNs);
    endNs = Math.max(endNs, node.endNs);
    maxDepth = Math.max(maxDepth, node.depth);
  }

  for (const node of tree) node.offsetNs = node.startNs - startNs;
  countDescendants(tree);

  return {
    traceId: trace.traceId,
    trace,
    roots,
    nodes: tree,
    byId: nodes,
    root,
    startNs,
    endNs,
    totalNs: Math.max(1, endNs - startNs),
    services: servicesOf(tree),
    maxDepth,
    orphanCount: tree.filter((node) => node.orphan).length,
    errorCount: tree.filter((node) => node.failed).length,
  };
}

function uniqueSpans(spans: readonly Span[] | undefined): Span[] {
  const seen = new Set<string>();
  const unique: Span[] = [];

  for (const span of spans ?? []) {
    if (!span.spanId || seen.has(span.spanId)) continue;
    seen.add(span.spanId);
    unique.push(filled(span));
  }

  return unique;
}

/** A span as the views read it: the lists and maps an older or partial answer leaves out are there, empty. */
function filled(span: Span): Span {
  if (span.attributes && span.events && span.links && span.resource && span.scope) return span;

  return {
    ...span,
    attributes: span.attributes ?? {},
    events: span.events ?? [],
    links: span.links ?? [],
    resource: span.resource ?? { attributes: {}, dropped_attributes_count: 0 },
    scope: span.scope ?? { name: '', version: '', attributes: {}, dropped_attributes_count: 0 },
  };
}

function nodeOf(span: Span): TraceNode {
  // An epoch in nanoseconds is past what a JSON number holds exactly, a
  // duration is not: a span's length is its own `duration_ns` where it has one.
  const startNs = Number(span.startNs) || 0;
  const ownDuration = Number(span.durationNs);
  const endNs = Number.isFinite(ownDuration) && ownDuration > 0
    ? startNs + ownDuration
    : Math.max(startNs, Number(span.endNs) || startNs);

  return {
    span,
    id: span.spanId,
    parentId: null,
    orphan: false,
    depth: 0,
    startNs,
    endNs,
    offsetNs: 0,
    durationNs: Math.max(1, endNs - startNs),
    selfNs: 0,
    children: [],
    descendants: 0,
    service: span.service || 'unknown',
    failed: span.status === 'ERROR',
  };
}

/** Stands a span as a root: its parent no longer leads to it. */
function detach(node: TraceNode, nodes: Map<string, TraceNode>): void {
  const parent = node.parentId ? nodes.get(node.parentId) : undefined;
  if (parent) parent.children = parent.children.filter((child) => child !== node);
  node.parentId = null;
  node.orphan = true;
}

function byStart(left: TraceNode, right: TraceNode): number {
  return left.startNs - right.startNs || left.span.operation.localeCompare(right.span.operation);
}

/** Lays out a span and those under it, each after its parent, without recursion: a trace can hold 100,000 spans. */
function walk(start: TraceNode, depth: number, out: TraceNode[], visited: Set<string>): void {
  const stack: Array<{ node: TraceNode; depth: number }> = [{ node: start, depth }];

  while (stack.length > 0) {
    const { node, depth: level } = stack.pop()!;
    if (visited.has(node.id)) continue;

    visited.add(node.id);
    node.depth = level;
    node.selfNs = selfTime(node);
    out.push(node);

    for (let index = node.children.length - 1; index >= 0; index -= 1) {
      const child = node.children[index]!;
      if (!visited.has(child.id)) stack.push({ node: child, depth: level + 1 });
    }
  }
}

/** Its duration less the time any span it called ran within it: calls that overlap are counted once. */
export function selfTime(node: TraceNode): number {
  const busy = node.children
    .map((child) => [Math.max(child.startNs, node.startNs), Math.min(child.endNs, node.endNs)] as const)
    .filter(([from, to]) => to > from)
    .sort((left, right) => left[0] - right[0]);

  let covered = 0;
  let reach = node.startNs;

  for (const [from, to] of busy) {
    const begin = Math.max(from, reach);
    if (to > begin) covered += to - begin;
    reach = Math.max(reach, to);
  }

  return Math.max(0, node.durationNs - covered);
}

/** `nodes` lists every span before those under it: counted from the last, each adds itself to its parent. */
function countDescendants(nodes: readonly TraceNode[]): void {
  const byId = new Map(nodes.map((node) => [node.id, node]));

  for (let index = nodes.length - 1; index >= 0; index -= 1) {
    const node = nodes[index]!;
    const parent = node.parentId ? byId.get(node.parentId) : undefined;
    if (parent) parent.descendants += node.descendants + 1;
  }
}

function servicesOf(nodes: readonly TraceNode[]): TraceService[] {
  const services = new Map<string, TraceService>();
  let work = 0;

  for (const node of nodes) {
    let service = services.get(node.service);
    if (!service) {
      service = { name: node.service, color: serviceColor(services.size), spans: 0, errors: 0, selfNs: 0, share: 0 };
      services.set(node.service, service);
    }

    service.spans += 1;
    service.selfNs += node.selfNs;
    if (node.failed) service.errors += 1;
    work += node.selfNs;
  }

  for (const service of services.values()) service.share = work > 0 ? service.selfNs / work : 0;

  return [...services.values()];
}

/** The colour each service of the tree is drawn in. */
export function serviceColors(tree: TraceTree): Map<string, string> {
  return new Map(tree.services.map((service) => [service.name, service.color]));
}

/** The spans from the trace's start down to `id`, the root first. */
export function pathTo(tree: TraceTree, id: string): TraceNode[] {
  const path: TraceNode[] = [];
  let node = tree.byId.get(id);

  while (node && !path.includes(node)) {
    path.unshift(node);
    node = node.parentId ? tree.byId.get(node.parentId) : undefined;
  }

  return path;
}
