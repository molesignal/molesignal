import type { TraceNode, TraceTree } from './tree';

/** The spans of one service, or of one operation of it, summed. */
export interface SpanGroup {
  key: string;
  service: string;
  /** `null` for a service's own line; the operation for a line under it. */
  operation: string | null;
  color: string;
  spans: number;
  errors: number;
  totalNs: number;
  avgNs: number;
  /** The time the spans spent on their own work. */
  selfNs: number;
  /** That time as a part of the work of the whole trace, from 0 to 1. */
  share: number;
  /** The operations of a service, the busiest first. */
  operations: SpanGroup[];
}

/**
 * The trace's spans summed by service and, under each, by operation: how
 * many there were, how long they took, and how much of the trace's own work
 * was theirs. The busiest come first.
 */
export function groupSpans(tree: TraceTree): SpanGroup[] {
  const work = tree.nodes.reduce((total, node) => total + node.selfNs, 0);
  const color = new Map(tree.services.map((service) => [service.name, service.color]));
  const services = new Map<string, { nodes: TraceNode[]; operations: Map<string, TraceNode[]> }>();

  for (const node of tree.nodes) {
    const service = services.get(node.service) ?? { nodes: [] as TraceNode[], operations: new Map<string, TraceNode[]>() };
    service.nodes.push(node);

    const operation = service.operations.get(node.span.operation) ?? [];
    operation.push(node);
    service.operations.set(node.span.operation, operation);
    services.set(node.service, service);
  }

  const groups = [...services.entries()].map(([name, service]) => ({
    ...summed(`${name}`, name, null, service.nodes, work, color.get(name) ?? ''),
    operations: [...service.operations.entries()]
      .map(([operation, nodes]) => summed(`${name}\u0000${operation}`, name, operation, nodes, work, color.get(name) ?? ''))
      .sort(busiestFirst),
  }));

  return groups.sort(busiestFirst);
}

function summed(
  key: string,
  service: string,
  operation: string | null,
  nodes: readonly TraceNode[],
  work: number,
  color: string,
): SpanGroup {
  const totalNs = nodes.reduce((total, node) => total + node.durationNs, 0);
  const selfNs = nodes.reduce((total, node) => total + node.selfNs, 0);

  return {
    key,
    service,
    operation,
    color,
    spans: nodes.length,
    errors: nodes.filter((node) => node.failed).length,
    totalNs,
    avgNs: nodes.length > 0 ? totalNs / nodes.length : 0,
    selfNs,
    share: work > 0 ? selfNs / work : 0,
    operations: [],
  };
}

function busiestFirst(left: SpanGroup, right: SpanGroup): number {
  return right.selfNs - left.selfNs || right.spans - left.spans || left.key.localeCompare(right.key);
}
