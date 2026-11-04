import { describe, expect, it } from 'vitest';

import type {
  MonitorDetail,
  MonitorRevision,
  SyntheticMonitor,
} from '@/api/synthetics';
import type { AlertRule } from '@/types/alerting';

import { alertRuleResources, syntheticMonitorResources } from './sourceResources';

describe('Status Page automation source resources', () => {
  it('uses alert rule names and stream targets and sorts them by name', () => {
    const resources = alertRuleResources([
      alertRule('rule-z', 'Zeta errors', false, 'logs', 'application'),
      alertRule('rule-a', 'API latency', true, 'metrics', 'http_server'),
    ]);

    expect(resources).toEqual([
      {
        id: 'rule-a',
        name: 'API latency',
        target: 'metrics / http_server',
        state: 'enabled',
      },
      {
        id: 'rule-z',
        name: 'Zeta errors',
        target: 'logs / application',
        state: 'disabled',
      },
    ]);
  });

  it('shows the published monitor target and excludes drafts and archived monitors', () => {
    const active = monitor('monitor-active', 'Checkout API', 'active', 'revision-active');
    const paused = monitor('monitor-paused', 'Login API', 'paused', 'revision-paused');
    const draft = monitor('monitor-draft', 'Draft check', 'draft');
    const archived = monitor('monitor-archived', 'Archived check', 'archived', 'revision-old');

    const resources = syntheticMonitorResources(
      [paused, archived, draft, active],
      [
        detail(active, [
          httpRevision(active, 'revision-draft', 'https://draft.example.com'),
          httpRevision(active, 'revision-active', 'https://api.example.com/health'),
        ]),
        detail(paused, [
          httpRevision(paused, 'revision-paused', 'https://login.example.com/health'),
        ]),
      ],
    );

    expect(resources).toEqual([
      {
        id: 'monitor-active',
        name: 'Checkout API',
        target: 'https://api.example.com/health',
        state: 'active',
      },
      {
        id: 'monitor-paused',
        name: 'Login API',
        target: 'https://login.example.com/health',
        state: 'paused',
      },
    ]);
  });
});

function alertRule(
  id: string,
  name: string,
  enabled: boolean,
  streamType: 'logs' | 'metrics' | 'traces',
  streamName: string,
): AlertRule {
  return {
    id,
    orgId: 'org-a',
    name,
    description: '',
    enabled,
    kind: 'scheduled',
    query: {
      language: 'sql',
      statement: 'SELECT 1',
      periodSecs: 60,
      stream: { name: streamName, streamType: streamType },
    },
    trigger: {
      operator: 'gt',
      threshold: 0,
      forPeriods: 1,
      silenceSecs: 0,
    },
    escalationPolicyId: '',
    labels: {},
    annotations: {},
  };
}

function monitor(
  id: string,
  name: string,
  lifecycle: SyntheticMonitor['lifecycle'],
  activeRevisionId?: string,
): SyntheticMonitor {
  return {
    id,
    organizationId: 'org-a',
    name,
    description: '',
    kind: 'http',
    lifecycle,
    state: 'unknown',
    tags: [],
    ...(activeRevisionId ? { activeRevisionId: activeRevisionId } : {}),
    createdBy: 'user-a',
    createdAt: 1,
    updatedAt: 1,
  };
}

function detail(value: SyntheticMonitor, revisions: MonitorRevision[]): MonitorDetail {
  return { monitor: value, revisions };
}

function httpRevision(
  value: SyntheticMonitor,
  id: string,
  url: string,
): MonitorRevision {
  return {
    id,
    organizationId: value.organizationId,
    monitorId: value.id,
    number: 1,
    spec: {
      kind: 'http',
      configuration: {
        steps: [{
          id: `${id}-step`,
          name: 'Health',
          method: 'GET',
          url: { source: 'literal', value: url },
          headers: [],
          query: [],
          extractions: [],
          assertions: [],
        }],
        followRedirects: true,
        maxRedirects: 5,
        verifyTls: true,
      },
    },
    schedule: { kind: 'interval', everySeconds: 60 },
    timeoutMillis: 5_000,
    maxRetries: 0,
    consecutiveFailures: 3,
    consecutiveRecoveries: 2,
    freshnessSeconds: 300,
    locationPolicy: { kind: 'majority' },
    locationIds: ['builtin-local'],
    alertOnDegraded: false,
    alertOnFlaky: false,
    alertOnNoData: true,
    injectTraceContext: true,
    tracerouteOnFailure: true,
    createdBy: 'user-a',
    createdAt: 1,
    contentHash: id,
  };
}
