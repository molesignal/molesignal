import { describe, expect, it } from 'vitest';

import type { RcaEvidence } from '@/types/alerting';

import { citedLabels, evidenceHref, insightFigure } from './rcaEvidence';

const piece = (overrides: Partial<RcaEvidence>): RcaEvidence => ({ label: 'E1', kind: 'insight', source: 'apm', ...overrides });

describe('rca evidence', () => {
  it('opens each piece where its link points', () => {
    expect(evidenceHref(piece({ kind: 'sample_trace', link: { traceId: 'abc' } }))).toBe('/traces/abc');
    expect(evidenceHref(piece({ kind: 'similar_incident', link: { incidentId: 'inc-0' } }))).toBe('/alerts/incidents/inc-0');
    expect(evidenceHref(piece({ kind: 'new_error_group', service: 'checkout', link: { namespace: 'shop', environment: 'prod', fingerprint: 'fp-1' } }))).toBe(
      '/apm/errors?namespace=shop&environment=prod&error=fp-1',
    );
    expect(evidenceHref(piece({ service: 'checkout', link: { namespace: 'shop', environment: 'prod' } }))).toBe('/apm/services/checkout?namespace=shop&environment=prod');
    expect(evidenceHref(piece({ kind: 'investigation', link: { route: '/logs?stream=default' } }))).toBe('/logs?stream=default');
  });

  it('opens logs over the window and nothing for an outside address', () => {
    const href = evidenceHref(piece({ kind: 'error_logs', link: { stream: 'default', from: 0, to: 60_000_000 } }));
    expect(href).toContain('/logs?stream=default&from=1970-01-01T00%3A00%3A00.000Z&to=1970-01-01T00%3A01%3A00.000Z');
    expect(evidenceHref(piece({ kind: 'investigation', link: { route: '//evil.example.com' } }))).toBeNull();
    expect(evidenceHref(piece({}))).toBeNull();
  });

  it('writes an insight figure in the unit of what moved', () => {
    expect(insightFigure('anomaly.p95', 820_000, 'en-us')).toBe('820 ms');
    expect(insightFigure('anomaly.error_rate', 0.0825, 'en-us')).toBe('8.25%');
    expect(insightFigure('slow_dependency', 0.62, 'en-us')).toBe('0.62');
  });

  it('reads the labels a summary cites once each', () => {
    expect(citedLabels('Pool ran out [E2], see [E1] and [E2].')).toEqual(['E2', 'E1']);
  });
});
