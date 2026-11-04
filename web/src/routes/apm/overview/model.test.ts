import { describe, expect, it } from 'vitest';

import type { Incident } from '@/types/alerting';

import { apdexTone, errorRateTone, healthSeries, incidentService, serviceIncidents } from './model';

function incident(patch: Partial<Incident>): Incident {
  return {
    id: 'i',
    orgId: 'o',
    ruleId: 'r',
    escalationPolicyId: 'e',
    status: 'open',
    severity: 'warning',
    summary: 's',
    fingerprint: 'f',
    currentStep: 0,
    currentLoop: 0,
    currentStepStartedAt: 0,
    assignees: [],
    createdAt: 0,
    labels: {},
    annotations: {},
    traceIds: [],
    hostIds: [],
    affectedServices: [],
    triggeringQuery: null,
    ...patch,
  } as Incident;
}

describe('the APM overview', () => {
  it('reads Apdex and error rates in the tones of the health lines', () => {
    expect([0.95, 0.8, 0.5, null].map(apdexTone)).toEqual(['good', 'warn', 'danger', undefined]);
    expect(errorRateTone(0.001, 100)).toBe('good');
    expect(errorRateTone(0.05, 100)).toBe('warn');
    expect(errorRateTone(0.2, 100)).toBe('danger');
    expect(errorRateTone(0, 0)).toBeUndefined();
  });

  it('finds the service an alert is about, and keeps only the ones going on', () => {
    expect(incidentService(incident({ labels: { 'service.name': 'checkout' } }))).toBe('checkout');
    expect(incidentService(incident({ affectedServices: ['billing'] }))).toBe('billing');
    expect(incidentService(incident({}))).toBeNull();

    const listed = serviceIncidents([
      incident({ id: 'a', severity: 'warning', createdAt: 3, labels: { service: 'cart' } }),
      incident({ id: 'b', severity: 'critical', createdAt: 1, labels: { service: 'pay' } }),
      incident({ id: 'c', severity: 'critical', status: 'resolved', labels: { service: 'pay' } }),
      incident({ id: 'd', severity: 'critical' }),
    ]);
    expect(listed.map((item) => item.incident.id)).toEqual(['b', 'a']);
  });

  it('draws the health trend as a series per state', () => {
    const series = healthSeries(
      [{ bucketAt: 1, counts: { critical: 1, warning: 0, healthy: 3, no_traffic: 0 } }],
      (health) => health,
    );
    expect(series.map((item) => [item.id, item.data[0]])).toEqual([
      ['critical', 1],
      ['warning', 0],
      ['healthy', 3],
      ['no_traffic', 0],
    ]);
  });
});
