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
        sampleValues: [],
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
        sampleValues: [],
      }),
      rule('traces'),
      time,
    );
    const trigger = new URL(links.find((link) => link.id === 'trigger')!.to, 'https://molesignal.test');
    expect(trigger.pathname).toBe('/traces');
    expect(trigger.searchParams.get('sql')).toBe('SELECT * FROM traces');
    expect(trigger.searchParams.get('streamType')).toBe('traces');
  });

  it('adds related Logs and Metrics pivots for an affected service', () => {
    const ids = buildIncidentInvestigationLinks(incident(null), undefined, time)
      .map((link) => link.id);
    expect(ids).toEqual(['logs', 'metrics']);
  });

  it('opens an incident of a RUM rule on the page of RUM that tells its figure, and offers nothing else', () => {
    const rum = {
      ...rule('logs'),
      query: {
        language: 'sql' as const,
        statement: 'rum:lcp_p75 storefront',
        periodSecs: 3_600,
        stream: null,
        rum: { applicationId: 'storefront', environment: 'prod', metric: 'lcp_p75' as const },
      },
    };
    const links = buildIncidentInvestigationLinks(
      {
        ...incident({ language: 'sql', statement: 'rum:lcp_p75 storefront', sampleValues: [] }),
        labels: { 'rum.application': 'storefront', 'deployment.environment': 'prod' },
        affectedServices: [],
      },
      rum,
      time,
    );

    expect(links.map((link) => link.id)).toEqual(['rum']);

    const open = new URL(links[0]!.to, 'https://molesignal.test');

    expect(open.pathname).toBe('/rum/performance');
    expect(open.searchParams.get('app')).toBe('storefront');
    expect(open.searchParams.get('env')).toBe('prod');
    expect(open.searchParams.get('vital')).toBe('lcp');
  });
});

function incident(
  triggeringQuery: Incident['triggeringQuery'],
): Incident {
  return {
    id: 'incident-1',
    orgId: 'org-1',
    ruleId: 'rule-1',
    escalationPolicyId: '',
    status: 'resolved',
    severity: 'warning',
    summary: 'Checkout errors',
    fingerprint: 'fingerprint',
    currentStep: 0,
    currentLoop: 0,
    currentStepStartedAt: 1,
    assignees: [],
    createdAt: Date.parse(time.from) * 1_000,
    resolvedAt: Date.parse(time.to) * 1_000,
    labels: { service: 'checkout' },
    annotations: {},
    traceIds: [],
    hostIds: [],
    affectedServices: ['checkout'],
    triggeringQuery: triggeringQuery,
  };
}

function rule(streamType: 'metrics' | 'logs' | 'traces'): AlertRule {
  return {
    id: 'rule-1',
    orgId: 'org-1',
    name: 'Checkout errors',
    description: '',
    enabled: true,
    query: {
      language: streamType === 'metrics' ? 'promql' : 'sql',
      statement: 'query',
      periodSecs: 60,
      stream: { name: `${streamType}-stream`, streamType: streamType },
    },
    trigger: {
      operator: 'gt',
      threshold: 1,
      forPeriods: 1,
      silenceSecs: 300,
    },
    escalationPolicyId: '',
    labels: {},
    annotations: {},
  };
}
