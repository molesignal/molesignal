import { describe, expect, it } from 'vitest';

import type { RedSummary } from '@/api/apm';

import {
  apmDefaultThreshold,
  apmDraftFromSearch,
  apmMetricUnit,
  apmMetricValue,
  apmNewErrorsPreview,
  apmNewRuleHref,
  apmPreviewResult,
  apmRuleLabels,
  apmRuleQuery,
} from './apmSource';

const red: RedSummary = {
  requestCount: 600,
  errorCount: 30,
  errorRate: 0.05,
  durationSum: 0,
  p95Time: 420_000,
  apdex: 0.9,
  latencyPartial: false,
  exemplars: [],
};
const idle: RedSummary = { requestCount: 0, errorCount: 0, errorRate: 0, durationSum: 0, latencyPartial: false, exemplars: [] };

describe('APM alert source', () => {
  it('opens on what an APM page asked for, and on nothing else', () => {
    const href = apmNewRuleHref({ namespace: 'shop', service: 'checkout', environment: 'prod', endpoint: { name: 'POST /orders', kind: 'rpc' }, metric: 'p95' });

    expect(apmDraftFromSearch(href.slice(href.indexOf('?')))).toEqual({
      service: 'checkout',
      namespace: 'shop',
      environment: 'prod',
      transaction: { name: 'POST /orders', kind: 'rpc' },
      metric: 'p95',
    });
    expect(apmDraftFromSearch('?source=apm&service=checkout&metric=slo_burn_rate&kind=queue&endpoint=x')).toMatchObject({
      metric: 'error_rate',
      namespace: null,
      environment: null,
      transaction: { name: 'x', kind: 'http' },
    });
    expect(apmDraftFromSearch('?service=checkout')).toBeNull();
  });

  it('saves a rule with no stream and labels its incidents with the service', () => {
    const draft = { service: ' checkout ', namespace: 'shop', environment: null, transaction: { name: 'POST /orders', kind: 'http' as const }, metric: 'error_rate' as const };

    expect(apmRuleQuery(draft, 300)).toEqual({
      language: 'sql',
      statement: '',
      periodSecs: 300,
      stream: null,
      apm: { ...draft, service: 'checkout' },
    });
    expect(apmRuleLabels({ ...draft, service: 'checkout' })).toEqual({
      'service.name': 'checkout',
      'service.namespace': 'shop',
      'apm.endpoint': 'POST /orders',
    });
  });

  it('reads figures in the unit thresholds are set in, with counts at zero once traffic stops', () => {
    expect(apmMetricValue('error_rate', red, 60)).toBe(5);
    expect(apmMetricValue('p95', red, 60)).toBe(420);
    expect(apmMetricValue('throughput', red, 60)).toBe(10);
    expect(apmMetricValue('apdex', red, 60)).toBe(0.9);
    expect(apmMetricValue('p99', red, 60)).toBeNull();

    expect(apmMetricValue('request_count', idle, 60)).toBe(0);
    expect(apmMetricValue('throughput', idle, 60)).toBe(0);
    expect(apmMetricValue('error_count', idle, 60)).toBe(0);
    expect(apmMetricValue('error_rate', idle, 60)).toBeNull();
    expect(apmMetricValue('apdex', idle, 60)).toBeNull();
  });

  it('previews a trend as the rows of a query, leaving out buckets without a figure', () => {
    const result = apmPreviewResult(
      [
        { bucketAt: 1, red },
        { bucketAt: 2, red: idle },
      ],
      'error_rate',
      60,
      7,
    );

    expect(result).toEqual({ columns: ['_timestamp', 'value'], rows: [[1, 5]], scannedRows: 1, tookMs: 7 });
  });

  it('counts the error groups first seen in each bucket, and pages on the first new one', () => {
    const trend = [
      { bucketAt: 0, red: idle },
      { bucketAt: 60_000_000, red: idle },
    ];

    expect(apmNewErrorsPreview(trend, [{ firstSeenAt: 10 }, { firstSeenAt: 60_000_000 }, { firstSeenAt: 61_000_000 }, { firstSeenAt: -5 }], 60, 3).rows).toEqual([
      [0, 1],
      [60_000_000, 2],
    ]);
    expect(apmMetricUnit('new_error_groups')).toBe('count');
    expect(apmDefaultThreshold('new_error_groups')).toEqual({ operator: 'gt', threshold: 0 });
  });
});
