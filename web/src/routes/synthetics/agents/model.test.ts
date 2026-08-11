import { describe, expect, it } from 'vitest';

import type { ProbeAgent, ProbeLocation } from '@/api/synthetics';

import {
  filterAgentRows,
  isReservedAgentLabel,
  joinAgentLocations,
  summarizeAgents,
  validateAgentConfiguration,
} from './model';

function agent(id: string, status: ProbeAgent['status'], available: number): ProbeAgent {
  return {
    id,
    location_id: id === 'edge' ? 'private-sg' : 'builtin-local',
    name: id,
    hostname: `${id}.example.test`,
    status,
    agent_version: '0.1.0',
    protocol_version: 1,
    capabilities: ['http'],
    capacity: {
      max_concurrent: 8,
      max_browser_concurrent: 2,
      available,
      available_browser: Math.min(available, 2),
    },
    labels: { region: id === 'edge' ? 'sg' : 'local' },
    last_result_sequence: 0,
    created_at: 1,
    updated_at: 1,
  };
}

function location(id: string, name: string): ProbeLocation {
  return {
    id,
    name,
    code: id,
    description: '',
    scope: id === 'builtin-local' ? 'platform' : 'organization',
    execution: id === 'builtin-local' ? 'embedded' : 'agent_pool',
    lifecycle: 'active',
    health: 'online',
    system_managed: id === 'builtin-local',
    egress_policy: {
      allowed_cidrs: [],
      denied_cidrs: [],
      allowed_domains: [],
      denied_domains: [],
      allowed_ports: [],
      allow_private_networks: false,
      allow_loopback: false,
    },
    created_at: 1,
    updated_at: 1,
  };
}

describe('Agent inventory model', () => {
  it('joins locations and searches labels, hostnames, and location names', () => {
    const rows = joinAgentLocations(
      [agent('local', 'online', 5), agent('edge', 'degraded', 1)],
      [location('builtin-local', 'Local'), location('private-sg', 'Singapore IDC')],
    );

    expect(filterAgentRows(rows, { query: 'singapore', status: 'all', locationId: 'all' }))
      .toHaveLength(1);
    expect(filterAgentRows(rows, { query: 'region sg', status: 'degraded', locationId: 'all' }))
      .toHaveLength(1);
  });

  it('excludes revoked agents from operational capacity', () => {
    const summary = summarizeAgents([
      agent('local', 'online', 5),
      agent('edge', 'degraded', 1),
      agent('retired', 'revoked', 8),
    ]);

    expect(summary).toMatchObject({ registered: 3, active: 2, online: 1, attention: 1, revoked: 1 });
    expect(summary.capacity).toEqual({ available: 6, maximum: 16 });
  });

  it('validates editable configuration after trimming label keys', () => {
    expect(
      validateAgentConfiguration('Local Probe', [
        { key: ' environment ', value: 'production' },
        { key: 'environment', value: 'staging' },
      ]),
    ).toBe('duplicate_label');
    expect(validateAgentConfiguration('Local Probe', [], 2)).toBeUndefined();
    expect(isReservedAgentLabel(' system_managed ')).toBe(true);
    expect(
      validateAgentConfiguration('Local Probe', [
        { key: 'execution', value: 'external' },
      ]),
    ).toBe('reserved_label');
  });
});
