import { describe, expect, it } from 'vitest';

import type { AlertRule, Incident } from '@/types/alerting';

import { adaptRule } from './alertRuleModel';

const baseRule: AlertRule = {
  id: 'rule-1',
  org_id: 'org-1',
  name: 'Checkout errors',
  description: '',
  enabled: true,
  query: {
    language: 'promql',
    statement: 'sum(rate(http_requests_total[5m]))',
    period_secs: 60,
    stream: { name: 'http_requests_total', stream_type: 'metrics' },
  },
  trigger: {
    operator: 'gt',
    threshold: 1,
    for_periods: 5,
    silence_secs: 300,
  },
  escalation_policy_id: '',
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
        { ...baseRule, last_eval_at: 1_723_000_000_000_000 },
        [],
      ).state,
    ).toBe('healthy');
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
    org_id: 'org-1',
    rule_id: ruleId,
    escalation_policy_id: '',
    status: 'open',
    severity: 'warning',
    summary: 'Checkout errors',
    fingerprint: 'fingerprint',
    current_step: 0,
    current_loop: 0,
    current_step_started_at: 1,
    assignees: [],
    created_at: 1,
    labels: {},
    annotations: {},
    trace_ids: [],
    host_ids: [],
    affected_services: [],
    triggering_query: null,
  };
}
