import { describe, expect, it } from 'vitest';

import type { NodeAgent } from '@/api/nodeAgents';

import { countByStatus, filterNodes, parseLabels, taskSubject } from './model';

function node(overrides: Partial<NodeAgent>): NodeAgent {
  return {
    id: 'node-1',
    orgId: 'org-a',
    hostId: 'host-1',
    bootId: '',
    hostname: 'web-1',
    labels: {},
    agentVersion: '0.4.0',
    protocolVersion: '1.0',
    capabilities: { taskKinds: ['tool'], features: [], maxRunningTasks: 2 },
    certificateSerial: '01',
    certificateExpiresAt: 0,
    enrollmentTokenId: null,
    health: { state: 'healthy', messages: [] },
    collectorStates: [],
    appliedConfigRevision: 0,
    desiredConfig: null,
    configStatus: null,
    lastHeartbeatAt: null,
    lastConnectedAt: null,
    drainRequestedAt: null,
    drainDeadline: null,
    drainedAt: null,
    revokedAt: null,
    revokeReason: null,
    enrolledAt: 0,
    createdAt: 0,
    updatedAt: 0,
    status: 'online',
    ...overrides,
  };
}

describe('node agents model', () => {
  const nodes = [
    node({ id: 'a', hostname: 'web-1', labels: { rack: 'r1' } }),
    node({ id: 'b', hostname: 'db-1', status: 'offline', agentVersion: '0.3.9' }),
    node({ id: 'c', hostname: '', hostId: 'machine-c', status: 'revoked' }),
  ];

  it('filters by status and by name, host, version or label', () => {
    expect(filterNodes(nodes, { query: '', status: 'offline' }).map((n) => n.id)).toEqual(['b']);
    expect(filterNodes(nodes, { query: 'rack=r1', status: 'all' }).map((n) => n.id)).toEqual(['a']);
    expect(filterNodes(nodes, { query: '0.3.9', status: 'all' }).map((n) => n.id)).toEqual(['b']);
    expect(filterNodes(nodes, { query: 'MACHINE', status: 'all' }).map((n) => n.id)).toEqual(['c']);
  });

  it('counts every status, the empty ones too', () => {
    const counts = countByStatus(nodes);
    expect(counts.online).toBe(1);
    expect(counts.offline).toBe(1);
    expect(counts.revoked).toBe(1);
    expect(counts.draining).toBe(0);
  });

  it('reads labels one key=value to a line and names the first bad line', () => {
    expect(parseLabels('rack = r1\n\nzone=eu-1\n')).toEqual({ labels: { rack: 'r1', zone: 'eu-1' } });
    expect(parseLabels('rack=r1\n=oops')).toEqual({ invalidLine: '=oops' });
    expect(parseLabels('no separator')).toEqual({ invalidLine: 'no separator' });
  });

  it('names a task by its tool, else its kind', () => {
    expect(taskSubject({ kind: 'tool', spec: { tool: 'tcpshark' } })).toBe('tcpshark');
    expect(taskSubject({ kind: 'profile', spec: {} })).toBe('profile');
  });
});
