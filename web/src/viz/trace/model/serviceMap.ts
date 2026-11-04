import type { TraceTree } from './tree';

/** A service of the trace, as its map draws it. */
export interface TraceMapService {
  name: string;
  color: string;
  spans: number;
  errors: number;
  /** Its part of the trace's own work, from 0 to 1. */
  share: number;
  /** The trace enters here: its root's service. */
  entry: boolean;
}

/** The calls one service made to another within the trace. */
export interface TraceMapCall {
  from: string;
  to: string;
  calls: number;
  /** Calls whose span in the called service failed. */
  errors: number;
  /** How long the called spans took, summed. */
  durationNs: number;
}

/**
 * The services of a trace and the calls between them: a span whose parent is
 * in another service is a call from that service to its own. Calls within a
 * service are its own work, not a line of the map.
 */
export function traceServiceMap(tree: TraceTree): { services: TraceMapService[]; calls: TraceMapCall[] } {
  const services = tree.services.map((service) => ({
    name: service.name,
    color: service.color,
    spans: service.spans,
    errors: service.errors,
    share: service.share,
    entry: service.name === tree.root.service,
  }));

  const calls = new Map<string, TraceMapCall>();

  for (const node of tree.nodes) {
    const parent = node.parentId ? tree.byId.get(node.parentId) : undefined;
    if (!parent || parent.service === node.service) continue;

    const key = `${parent.service}\u0000${node.service}`;
    const call = calls.get(key) ?? { from: parent.service, to: node.service, calls: 0, errors: 0, durationNs: 0 };
    call.calls += 1;
    call.durationNs += node.durationNs;
    if (node.failed) call.errors += 1;
    calls.set(key, call);
  }

  return { services, calls: [...calls.values()] };
}
