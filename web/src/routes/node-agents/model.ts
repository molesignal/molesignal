import type { NodeAgent, NodeStatus, NodeTask } from '@/api/nodeAgents';

export const NODE_STATUSES: readonly NodeStatus[] = [
  'online',
  'offline',
  'enrolled',
  'draining',
  'drained',
  'revoked',
];

/** How much a status wants attention: offline first, revoked last. */
const STATUS_ORDER: Record<NodeStatus, number> = {
  offline: 0,
  draining: 1,
  drained: 2,
  enrolled: 3,
  online: 4,
  revoked: 5,
};

export function statusOrder(status: NodeStatus): number {
  return STATUS_ORDER[status];
}

export function nodeName(node: Pick<NodeAgent, 'hostname' | 'hostId'>): string {
  return node.hostname || node.hostId;
}

/** Nodes whose name, host id, version or labels hold the query, of one status or all. */
export function filterNodes(
  nodes: readonly NodeAgent[],
  { query, status }: { query: string; status: NodeStatus | 'all' },
): NodeAgent[] {
  const needle = query.trim().toLowerCase();

  return nodes.filter((node) => {
    if (status !== 'all' && node.status !== status) return false;
    if (!needle) return true;

    const haystack = [
      node.hostname,
      node.hostId,
      node.id,
      node.agentVersion,
      ...Object.entries(node.labels).map(([key, value]) => `${key}=${value}`),
    ];

    return haystack.some((value) => value.toLowerCase().includes(needle));
  });
}

/** How many nodes stand in each status. */
export function countByStatus(nodes: readonly NodeAgent[]): Record<NodeStatus, number> {
  const counts = Object.fromEntries(NODE_STATUSES.map((status) => [status, 0])) as Record<
    NodeStatus,
    number
  >;

  for (const node of nodes) counts[node.status] += 1;
  return counts;
}

/** Labels written one `key=value` to a line. Returns the labels, or the first line that is not one. */
export function parseLabels(
  text: string,
): { labels: Record<string, string> } | { invalidLine: string } {
  const labels: Record<string, string> = {};

  for (const raw of text.split('\n')) {
    const line = raw.trim();
    if (!line) continue;

    const separator = line.indexOf('=');
    const key = separator > 0 ? line.slice(0, separator).trim() : '';

    if (!key || key.length > 63) return { invalidLine: line };
    labels[key] = line.slice(separator + 1).trim();
  }

  return { labels };
}

/** A spec typed as JSON: the object, or why it is not one. */
export function parseSpec(text: string): { spec: Record<string, unknown> } | { error: 'json' | 'object' } {
  let value: unknown;

  try {
    value = JSON.parse(text);
  } catch {
    return { error: 'json' };
  }

  if (!value || typeof value !== 'object' || Array.isArray(value)) return { error: 'object' };
  return { spec: value as Record<string, unknown> };
}

/** What a task is in a few words: the tool it runs, else its kind. */
export function taskSubject(task: Pick<NodeTask, 'kind' | 'spec'>): string {
  const tool = task.spec.tool;
  return typeof tool === 'string' && tool ? tool : task.kind;
}
