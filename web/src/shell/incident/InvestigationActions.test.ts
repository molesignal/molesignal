import { describe, expect, it } from 'vitest';

import type { AlertRule, Incident } from '@/types/alerting';

import {
  buildIncidentInvestigationLinks,
  type IncidentTimeWindow,
} from './InvestigationActions';

const time: IncidentTimeWindow = {
  from: '2026-08-14T05:00:00.000Z',
  to: '2026-08-14T06:00:00.000Z',
};

describe('incident investigation links', () => {
  it('opens a PromQL trigger in Metrics and preserves the incident window', () => {
    const links = buildIncidentInvestigationLinks(
      incident({
        language: 'promql',
        statement: 'sum(rate(http_requests_total[5m]))',
        sample_values: [],
      }),
      rule('metrics'),
      time,
    );
    const trigger = new URL(links.find((link) => link.id === 'trigger')!.to, 'https://molesignal.test');
    expect(trigger.pathname).toBe('/metrics');
    expect(trigger.searchParams.get('promql')).toBe(
      'sum(rate(http_requests_total[5m]))',
    );
    expect(trigger.searchParams.get('time')).toBe(`${time.from}..${time.to}`);
  });

  it('opens a trace SQL trigger in Traces with the source stream', () => {
    const links = buildIncidentInvestigationLinks(
      incident({
        language: 'sql',
        statement: 'SELECT * FROM traces',
        sample_values: [],
      }),
      rule('traces'),
      time,
    );
    const trigger = new URL(links.find((link) => link.id === 'trigger')!.to, 'https://molesignal.test');
    expect(trigger.pathname).toBe('/traces');
    expect(trigger.searchParams.get('sql')).toBe('SELECT * FROM traces');
    expect(trigger.searchParams.get('stream_type')).toBe('traces');
  });

  it('adds related Logs and Metrics pivots for an affected service', () => {
    const ids = buildIncidentInvestigationLinks(incident(null), undefined, time)
      .map((link) => link.id);
    expect(ids).toEqual(['logs', 'metrics']);
  });
});

function incident(
  triggeringQuery: Incident['triggering_query'],
): Incident {
  return {
    id: 'incident-1',
    org_id: 'org-1',
    rule_id: 'rule-1',
    escalation_policy_id: '',
    status: 'resolved',
    severity: 'warning',
    summary: 'Checkout errors',
    fingerprint: 'fingerprint',
    current_step: 0,
    current_loop: 0,
    current_step_started_at: 1,
    assignees: [],
    created_at: Date.parse(time.from) * 1_000,
    resolved_at: Date.parse(time.to) * 1_000,
    labels: { service: 'checkout' },
    annotations: {},
    trace_ids: [],
    host_ids: [],
    affected_services: ['checkout'],
    triggering_query: triggeringQuery,
  };
}

function rule(streamType: 'metrics' | 'logs' | 'traces'): AlertRule {
  return {
    id: 'rule-1',
    org_id: 'org-1',
    name: 'Checkout errors',
    description: '',
    enabled: true,
    query: {
      language: streamType === 'metrics' ? 'promql' : 'sql',
      statement: 'query',
      period_secs: 60,
      stream: { name: `${streamType}-stream`, stream_type: streamType },
    },
    trigger: {
      operator: 'gt',
      threshold: 1,
      for_periods: 1,
      silence_secs: 300,
    },
    escalation_policy_id: '',
    labels: {},
    annotations: {},
  };
}
