import { describe, expect, it } from 'vitest';

import type { CorrelationGrades, CorrelationReport, ServiceCorrelation } from '@/api/apmCorrelation';

import { correlationFindings, correlationLookup, correlationScore, someNames, tracedShare } from './model';

function entry(name: string, grades: Partial<CorrelationGrades>, overrides: Partial<ServiceCorrelation> = {}): ServiceCorrelation {
  return {
    service: { namespace: 'shop', name, environment: 'prod' },
    logs: { state: 'reported', streams: ['default'], lines: 100, traced: 100, sampled: 10, matched: 10 },
    metrics: 'reported',
    resource: { environment: true, version: true, instance: true, placement: true },
    changes: { count: 1, lastAt: 1 },
    grades: { traceIds: 'good', resource: 'good', metrics: 'good', changes: 'good', ...grades },
    ...overrides,
  };
}

function report(services: ServiceCorrelation[], unmatched: CorrelationReport['unmatched'] = []): CorrelationReport {
  return { from: 0, to: 1, changesSince: 0, services, unmatched };
}

describe('correlation model', () => {
  it('scores two a way that ties well and one a way that ties in part', () => {
    expect(correlationScore({ traceIds: 'good', resource: 'partial', metrics: 'missing', changes: 'unknown' })).toBe(3);
  });

  it('looks a service up by namespace, name and environment', () => {
    const lookup = correlationLookup(report([entry('checkout', {})]));

    expect(lookup({ namespace: 'shop', name: 'checkout', environment: 'prod' })?.service.name).toBe('checkout');
    expect(lookup({ namespace: 'shop', name: 'checkout', environment: 'staging' })).toBeNull();
    expect(lookup({ namespace: 'shop', name: 'checkout', environment: null })).toBeNull();
  });

  it('tells the traced share only of a service that writes logs', () => {
    expect(tracedShare(entry('a', {}, { logs: { state: 'reported', streams: [], lines: 4, traced: 1, sampled: 0, matched: 0 } }))).toBe(0.25);
    expect(tracedShare(entry('a', {}, { logs: { state: 'missing', streams: [], lines: 0, traced: 0, sampled: 0, matched: 0 } }))).toBeNull();
  });

  it('lists the fixes before the suggestions, each with its services', () => {
    const findings = correlationFindings(
      report(
        [
          entry('checkout', { traceIds: 'missing', changes: 'missing' }, { logs: { state: 'reported', streams: ['default'], lines: 10, traced: 0, sampled: 0, matched: 0 } }),
          entry('cart', { traceIds: 'partial' }, { resource: { environment: true, version: true, instance: true, placement: false } }),
          entry('quiet', { traceIds: 'missing' }, { logs: { state: 'missing', streams: [], lines: 0, traced: 0, sampled: 0, matched: 0 } }),
        ],
        [{ name: 'checkout-svc', signal: 'logs', streams: ['default'], lines: 50 }],
      ),
    );

    expect(findings.map((finding) => [finding.key, finding.names])).toEqual([
      ['untraced', ['checkout']],
      ['partly_traced', ['cart']],
      ['unplaced', ['cart']],
      ['unmatched', ['checkout-svc']],
      ['no_changes', ['checkout']],
    ]);
  });

  it('shows a few names and counts the rest', () => {
    expect(someNames(['a', 'b', 'c', 'd', 'e'])).toEqual({ shown: ['a', 'b', 'c'], more: 2 });
  });
});
