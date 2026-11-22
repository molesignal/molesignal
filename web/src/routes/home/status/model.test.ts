import { describe, expect, it } from 'vitest';

import type { OverviewResponse, ServiceSummary } from '@/api/apm';
import type { SyntheticMonitor } from '@/api/synthetics';
import type { AlertRule, Incident } from '@/types/alerting';

import {
  apmTrend,
  formatShare,
  rankServices,
  summarizeChecks,
  summarizeIncidents,
  summarizeRules,
  verdictOf,
} from './model';

function incident(id: string, severity: Incident['severity'], status: Incident['status'], createdAt: number): Incident {
  return { id, severity, status, createdAt, summary: id, affectedServices: [], assignees: [] } as unknown as Incident;
}

function rule(enabled: boolean, kind?: 'healthy' | 'pending' | 'firing'): AlertRule {
  const lastState = kind === 'pending' ? { kind, consecutive: 1 } : kind ? { kind } : undefined;
  return { enabled, lastState } as unknown as AlertRule;
}

function monitor(name: string, state: SyntheticMonitor['state'], lifecycle: SyntheticMonitor['lifecycle'] = 'active'): SyntheticMonitor {
  return { id: name, name, state, lifecycle } as SyntheticMonitor;
}

function service(name: string, health: ServiceSummary['health'], requestCount: number): ServiceSummary {
  return {
    service: { namespace: 'shop', name, environment: 'prod' },
    health,
    red: { requestCount },
  } as unknown as ServiceSummary;
}

describe('summarizeIncidents', () => {
  it('keeps what is going on, the most severe and then the oldest first', () => {
    const summary = summarizeIncidents([
      incident('warn-old', 'warning', 'open', 1),
      incident('crit-new', 'critical', 'acknowledged', 9),
      incident('crit-old', 'critical', 'open', 2),
      incident('done', 'critical', 'resolved', 0),
    ]);

    expect(summary.active.map((item) => item.id)).toEqual(['crit-old', 'crit-new', 'warn-old']);
    expect(summary.bySeverity).toEqual({ critical: 2, error: 0, warning: 1, info: 0 });
    expect(summary.unacknowledged).toBe(2);
  });
});

describe('summarizeRules', () => {
  it('counts the enabled rules by their last state', () => {
    expect(summarizeRules([rule(true, 'firing'), rule(true, 'pending'), rule(true, 'healthy'), rule(true), rule(false, 'firing')])).toEqual({
      enabled: 4,
      firing: 1,
      pending: 1,
    });
  });
});

describe('summarizeChecks', () => {
  it('counts the checks that run and lists the failing ones before the degraded and flaky', () => {
    const summary = summarizeChecks([
      monitor('b-flaky', 'flaky'),
      monitor('a-ok', 'healthy'),
      monitor('c-degraded', 'degraded'),
      monitor('d-failing', 'failing'),
      monitor('paused', 'failing', 'paused'),
    ]);

    expect(summary.active).toBe(4);
    expect(summary.byState.failing).toBe(1);
    expect(summary.attention.map((item) => item.name)).toEqual(['d-failing', 'c-degraded', 'b-flaky']);
  });
});

describe('rankServices', () => {
  it('puts unhealthy services first and the busiest next, cut to the limit', () => {
    const ranked = rankServices(
      [service('quiet', 'healthy', 5), service('busy', 'healthy', 50), service('down', 'critical', 1), service('slow', 'warning', 2)],
      3,
    );

    expect(ranked.map((item) => item.service.name)).toEqual(['down', 'slow', 'busy']);
  });
});

describe('apmTrend', () => {
  it('leaves a bucket without requests out of the rates', () => {
    const overview = {
      trend: [
        { bucketAt: 1, red: { requestCount: 10, errorRate: 0.1, p95Time: 2_000 } },
        { bucketAt: 2, red: { requestCount: 0, errorRate: 0, p95Time: 0 } },
      ],
    } as unknown as OverviewResponse;

    expect(apmTrend(overview)).toEqual({ errorRate: [0.1, null], p95: [2_000, null], requests: [10, 0] });
    expect(apmTrend(undefined).errorRate).toEqual([]);
  });
});

describe('verdictOf', () => {
  it('is red for anything severe, yellow for the rest and green with nothing to say', () => {
    const incidents = summarizeIncidents([incident('w', 'warning', 'open', 1)]);

    expect(verdictOf({ incidents })).toEqual({ issues: 1, tone: 'yellow' });
    expect(verdictOf({ incidents, checks: summarizeChecks([monitor('x', 'failing')]) })).toEqual({ issues: 2, tone: 'red' });
    expect(verdictOf({ signals: [{ status: 'delayed' }, { status: 'healthy' }] })).toEqual({ issues: 1, tone: 'yellow' });
    expect(verdictOf({})).toEqual({ issues: 0, tone: 'green' });
  });
});

describe('formatShare', () => {
  it('keeps small rates readable and large ones short', () => {
    expect(formatShare(0.0042)).toBe('0.42%');
    expect(formatShare(0.0523)).toBe('5.2%');
    expect(formatShare(0.5)).toBe('50.0%');
    expect(formatShare(0.9997)).toBe('99.97%');
    expect(formatShare(1)).toBe('100.0%');
    expect(formatShare(null)).toBe('—');
  });
});
