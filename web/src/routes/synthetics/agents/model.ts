import type { ProbeAgent, ProbeLocation } from '@/api/synthetics';

export type AgentStatus = ProbeAgent['status'];

export interface AgentRow {
  agent: ProbeAgent;
  location: ProbeLocation | undefined;
}

export interface AgentFilters {
  query: string;
  status: AgentStatus | 'all';
  locationId: string | 'all';
}

export interface AgentSummary {
  registered: number;
  active: number;
  online: number;
  attention: number;
  revoked: number;
  capacity: { available: number; maximum: number };
  browserCapacity: { available: number; maximum: number };
}

export interface AgentLabelInput {
  key: string;
  value: string;
}

export type AgentConfigurationError =
  | 'name_required'
  | 'name_too_long'
  | 'too_many_labels'
  | 'label_key_required'
  | 'label_key_too_long'
  | 'label_value_too_long'
  | 'reserved_label'
  | 'duplicate_label';

export const RESERVED_AGENT_LABEL_KEYS = ['execution', 'system_managed'] as const;

export const AGENT_STATUSES: readonly AgentStatus[] = [
  'registered',
  'online',
  'degraded',
  'offline',
  'draining',
  'revoked',
];

export function joinAgentLocations(
  agents: ProbeAgent[],
  locations: ProbeLocation[],
): AgentRow[] {
  const locationsById = new Map(locations.map((location) => [location.id, location]));
  return agents.map((agent) => ({ agent, location: locationsById.get(agent.location_id) }));
}

export function filterAgentRows(rows: AgentRow[], filters: AgentFilters): AgentRow[] {
  const query = filters.query.trim().toLocaleLowerCase();
  return rows.filter(({ agent, location }) => {
    const text = [
      agent.name,
      agent.hostname,
      agent.agent_version,
      location?.name,
      location?.code,
      ...agent.capabilities,
      ...Object.entries(agent.labels).flatMap(([key, value]) => [key, value]),
    ]
      .filter(Boolean)
      .join(' ')
      .toLocaleLowerCase();
    return (
      (!query || text.includes(query)) &&
      (filters.status === 'all' || agent.status === filters.status) &&
      (filters.locationId === 'all' || agent.location_id === filters.locationId)
    );
  });
}

export function summarizeAgents(agents: ProbeAgent[]): AgentSummary {
  const active = agents.filter((agent) => agent.status !== 'revoked');
  return {
    registered: agents.length,
    active: active.length,
    online: active.filter((agent) => agent.status === 'online').length,
    attention: active.filter((agent) => agent.status !== 'online').length,
    revoked: agents.length - active.length,
    capacity: sumCapacity(active, 'available', 'max_concurrent'),
    browserCapacity: sumCapacity(active, 'available_browser', 'max_browser_concurrent'),
  };
}

export function agentDisplayName(agent: ProbeAgent): string {
  return agent.name.trim() || agent.hostname;
}

export function isReservedAgentLabel(key: string): boolean {
  return RESERVED_AGENT_LABEL_KEYS.includes(
    key.trim() as (typeof RESERVED_AGENT_LABEL_KEYS)[number],
  );
}

export function validateAgentConfiguration(
  name: string,
  labels: AgentLabelInput[],
  reservedLabelCount = 0,
): AgentConfigurationError | undefined {
  const nameLength = Array.from(name.trim()).length;
  if (nameLength === 0) return 'name_required';
  if (nameLength > 255) return 'name_too_long';
  if (labels.length + reservedLabelCount > 32) return 'too_many_labels';

  const keys = new Set<string>();
  for (const label of labels) {
    const key = label.key.trim();
    if (!key) return 'label_key_required';
    if (Array.from(key).length > 64) return 'label_key_too_long';
    if (Array.from(label.value.trim()).length > 255) return 'label_value_too_long';
    if (isReservedAgentLabel(key)) return 'reserved_label';
    if (keys.has(key)) return 'duplicate_label';
    keys.add(key);
  }
  return undefined;
}

function sumCapacity(
  agents: ProbeAgent[],
  availableKey: 'available' | 'available_browser',
  maximumKey: 'max_concurrent' | 'max_browser_concurrent',
): { available: number; maximum: number } {
  return agents.reduce(
    (total, agent) => ({
      available: total.available + agent.capacity[availableKey],
      maximum: total.maximum + agent.capacity[maximumKey],
    }),
    { available: 0, maximum: 0 },
  );
}
