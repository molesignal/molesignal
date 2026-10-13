import { describe, expect, it } from 'vitest';

import type { Incident } from '@/types/alerting';

import {
  affectedServices,
  byUrgency,
  incidentDurationSecs,
  incidentRunbook,
  incidentService,
  isResolvedIncident,
  severityCounts,
} from './model';

const SECOND = 1_000_000;

function incident(overrides: Partial<Incident> = {}): Incident {
  return {
    id: 'incident-1',
    org_id: 'org-1',
    rule_id: 'rule-1',
    escalation_policy_id: 'policy-1',
    status: 'open',
    severity: 'warning',
    summary: 'Checkout latency is high',
    fingerprint: 'fp',
    current_step: 0,
    current_loop: 0,
    current_step_started_at: 0,
    assignees: [],
    created_at: 100 * SECOND,
    labels: {},
    annotations: {},
    trace_ids: [],
    host_ids: [],
    affected_services: [],
    triggering_query: null,
    ...overrides,
  };
}

describe('incident urgency', () => {
  it('puts the worst first and, among equals, the one that has burnt longest', () => {
    const sorted = [
      incident({ id: 'warning-old', severity: 'warning', created_at: 1 * SECOND }),
      incident({ id: 'critical-new', severity: 'critical', created_at: 50 * SECOND }),
      incident({ id: 'critical-old', severity: 'critical', created_at: 10 * SECOND }),
      incident({ id: 'error', severity: 'error', created_at: 5 * SECOND }),
    ]
      .sort(byUrgency)
      .map((item) => item.id);

    expect(sorted).toEqual(['critical-old', 'critical-new', 'error', 'warning-old']);
  });

  it('counts every severity, including the ones nothing has', () => {
    expect(
      severityCounts([
        incident({ severity: 'critical' }),
        incident({ severity: 'critical' }),
        incident({ severity: 'info' }),
      ]),
    ).toEqual({ critical: 2, error: 0, warning: 0, info: 1 });
  });
});

describe('incident facts', () => {
  it('treats resolved and closed as over', () => {
    expect(isResolvedIncident(incident({ status: 'resolved' }))).toBe(true);
    expect(isResolvedIncident(incident({ status: 'closed' }))).toBe(true);
    expect(isResolvedIncident(incident({ status: 'open' }))).toBe(false);
    expect(isResolvedIncident(incident({ status: 'acknowledged' }))).toBe(false);
  });

  it('names the service from the incident, then from its labels', () => {
    expect(
      incidentService(incident({ affected_services: ['checkout'], labels: { service: 'cart' } })),
    ).toBe('checkout');
    expect(incidentService(incident({ labels: { service: 'cart' } }))).toBe('cart');
    expect(incidentService(incident({ labels: { svc: 'payments' } }))).toBe('payments');
    expect(incidentService(incident())).toBeUndefined();
  });

  it('lists each affected service once, in the order they are met', () => {
    expect(
      affectedServices([
        incident({ affected_services: ['checkout', 'cart'] }),
        incident({ labels: { service: 'cart' } }),
        incident({ labels: { svc: 'payments' } }),
        incident(),
      ]),
    ).toEqual(['checkout', 'cart', 'payments']);
  });

  it('measures how long an incident has lasted, or lasted until it recovered', () => {
    expect(incidentDurationSecs(incident(), 160 * SECOND)).toBe(60);
    expect(incidentDurationSecs(incident({ resolved_at: 130 * SECOND }), 9_999 * SECOND)).toBe(30);
    // A clock behind the incident's does not make the duration negative.
    expect(incidentDurationSecs(incident(), 50 * SECOND)).toBe(0);
  });
});

describe('incident runbook', () => {
  it('takes it from the incident before the rule', () => {
    const rule = { runbook_url: 'https://wiki.example.com/rule' };

    expect(
      incidentRunbook(incident({ annotations: { runbook_url: 'https://wiki.example.com/incident' } }), rule),
    ).toBe('https://wiki.example.com/incident');
    expect(incidentRunbook(incident({ annotations: { runbook: 'http://wiki.example.com/r' } }), rule)).toBe(
      'http://wiki.example.com/r',
    );
    expect(incidentRunbook(incident(), rule)).toBe('https://wiki.example.com/rule');
    expect(incidentRunbook(incident(), undefined)).toBeUndefined();
  });

  it('is only an address the browser can open', () => {
    for (const unusable of ['javascript:alert(1)', 'wiki/checkout', 'ftp://wiki.example.com', '']) {
      expect(
        incidentRunbook(incident({ annotations: { runbook_url: unusable } }), undefined),
        unusable,
      ).toBeUndefined();
    }
  });
});
