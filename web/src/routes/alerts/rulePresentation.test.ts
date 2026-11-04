import { describe, expect, it } from 'vitest';

import type { AlertRule, Incident } from '@/types/alerting';

import { adaptRule } from './alertRuleModel';

const baseRule: AlertRule = {
  id: 'rule-1',
  orgId: 'org-1',
  name: 'Checkout errors',
  description: '',
  enabled: true,
  query: {
    language: 'promql',
    statement: 'sum(rate(http_requests_total[5m]))',
    periodSecs: 60,
    stream: { name: 'http_requests_total', streamType: 'metrics' },
  },
  trigger: {
    operator: 'gt',
    threshold: 1,
    forPeriods: 5,
    silenceSecs: 300,
  },
  escalationPolicyId: '',
  labels: { service: 'checkout' },
  annotations: {},
};

describe('adaptRule', () => {
  it('does not label an enabled rule healthy before its first evaluation', () => {
    expect(adaptRule(baseRule, []).state).toBe('not_evaluated');
  });

  it('labels a completed, non-firing rule healthy', () => {
    expect(
      adaptRule(
        { ...baseRule, lastEvalAt: 1_723_000_000_000_000 },
        [],
      ).state,
    ).toBe('healthy');
  });

  it('says when the rule fired but an alert pipeline dropped its event', () => {
    const dropped = { ...baseRule, lastEvalAt: 1_723_000_000_000_000, lastState: { kind: 'dropped' as const } };

    expect(adaptRule(dropped, []).state).toBe('dropped');
    // An incident another group of the rule opened still says it fires.
    expect(adaptRule(dropped, [incidentForRule(baseRule.id)]).state).toBe('firing');
  });

  it('says when every incident of a firing rule is held back by an inhibit rule', () => {
    const held = {
      ...incidentForRule(baseRule.id),
      inhibition: {
        ruleId: 'inhibit-1',
        ruleName: 'Same service',
        sourceIncidentId: 'inc-worse',
        sourceSummary: 'Database down',
        since: 1,
      },
    };

    expect(adaptRule(baseRule, [held]).state).toBe('inhibited');
    // One incident still notifying: the rule fires.
    expect(adaptRule(baseRule, [held, incidentForRule(baseRule.id)]).state).toBe('firing');
  });

  it('lets an active incident take precedence over missing evaluation metadata', () => {
    expect(adaptRule(baseRule, [incidentForRule(baseRule.id)]).state).toBe(
      'firing',
    );
  });
});

function incidentForRule(ruleId: string): Incident {
  return {
    id: 'incident-1',
    orgId: 'org-1',
    ruleId: ruleId,
    escalationPolicyId: '',
    status: 'open',
    severity: 'warning',
    summary: 'Checkout errors',
    fingerprint: 'fingerprint',
    currentStep: 0,
    currentLoop: 0,
    currentStepStartedAt: 1,
    assignees: [],
    createdAt: 1,
    labels: {},
    annotations: {},
    traceIds: [],
    hostIds: [],
    affectedServices: [],
    triggeringQuery: null,
  };
}
