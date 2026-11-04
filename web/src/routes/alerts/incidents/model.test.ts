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
    orgId: 'org-1',
    ruleId: 'rule-1',
    escalationPolicyId: 'policy-1',
    status: 'open',
    severity: 'warning',
    summary: 'Checkout latency is high',
    fingerprint: 'fp',
    currentStep: 0,
    currentLoop: 0,
    currentStepStartedAt: 0,
    assignees: [],
    createdAt: 100 * SECOND,
    labels: {},
    annotations: {},
    traceIds: [],
    hostIds: [],
    affectedServices: [],
    triggeringQuery: null,
    ...overrides,
  };
}

describe('incident urgency', () => {
  it('puts the worst first and, among equals, the one that has burnt longest', () => {
    const sorted = [
      incident({ id: 'warning-old', severity: 'warning', createdAt: 1 * SECOND }),
      incident({ id: 'critical-new', severity: 'critical', createdAt: 50 * SECOND }),
      incident({ id: 'critical-old', severity: 'critical', createdAt: 10 * SECOND }),
      incident({ id: 'error', severity: 'error', createdAt: 5 * SECOND }),
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
      incidentService(incident({ affectedServices: ['checkout'], labels: { service: 'cart' } })),
    ).toBe('checkout');
    expect(incidentService(incident({ labels: { service: 'cart' } }))).toBe('cart');
    expect(incidentService(incident({ labels: { svc: 'payments' } }))).toBe('payments');
    expect(incidentService(incident())).toBeUndefined();
  });

  it('lists each affected service once, in the order they are met', () => {
    expect(
      affectedServices([
        incident({ affectedServices: ['checkout', 'cart'] }),
        incident({ labels: { service: 'cart' } }),
        incident({ labels: { svc: 'payments' } }),
        incident(),
      ]),
    ).toEqual(['checkout', 'cart', 'payments']);
  });

  it('measures how long an incident has lasted, or lasted until it recovered', () => {
    expect(incidentDurationSecs(incident(), 160 * SECOND)).toBe(60);
    expect(incidentDurationSecs(incident({ resolvedAt: 130 * SECOND }), 9_999 * SECOND)).toBe(30);
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
