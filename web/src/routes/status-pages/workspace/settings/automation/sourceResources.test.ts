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
    org_id: 'org-a',
    name,
    description: '',
    enabled,
    kind: 'scheduled',
    query: {
      language: 'sql',
      statement: 'SELECT 1',
      period_secs: 60,
      stream: { name: streamName, stream_type: streamType },
    },
    trigger: {
      operator: 'gt',
      threshold: 0,
      for_periods: 1,
      silence_secs: 0,
    },
    escalation_policy_id: '',
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
    organization_id: 'org-a',
    name,
    description: '',
    kind: 'http',
    lifecycle,
    state: 'unknown',
    tags: [],
    ...(activeRevisionId ? { active_revision_id: activeRevisionId } : {}),
    created_by: 'user-a',
    created_at: 1,
    updated_at: 1,
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
    organization_id: value.organization_id,
    monitor_id: value.id,
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
        follow_redirects: true,
        max_redirects: 5,
        verify_tls: true,
      },
    },
    schedule: { kind: 'interval', every_seconds: 60 },
    timeout_millis: 5_000,
    max_retries: 0,
    consecutive_failures: 3,
    consecutive_recoveries: 2,
    freshness_seconds: 300,
    location_policy: { kind: 'majority' },
    location_ids: ['builtin-local'],
    alert_on_degraded: false,
    created_by: 'user-a',
    created_at: 1,
    content_hash: id,
  };
}
