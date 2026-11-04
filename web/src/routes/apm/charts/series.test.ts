import { describe, expect, it } from 'vitest';

import type { TrendPoint, VersionSummary } from '@/api/apm';

import { bucketSeconds, deployAnnotations, errorRateSeries, requestSeries, shiftTrend } from './series';

const labels = { ok: 'ok', errors: 'errors', previous: 'before', errorRate: 'rate', p50: 'P50', p95: 'P95', p99: 'P99' };

function point(at: number, requests: number, errors: number): TrendPoint {
  return {
    bucketAt: at,
    red: { requestCount: requests, errorCount: errors, errorRate: requests ? errors / requests : 0, durationSum: 0, latencyPartial: false, exemplars: [] },
  };
}

describe('APM chart series', () => {
  it('stacks failed requests on the others a second at a time, the compared total dashed', () => {
    const series = requestSeries([point(0, 120, 6)], [point(0, 60, 0)], bucketSeconds('minute'), labels);
    expect(series.map((entry) => [entry.id, entry.data[0]])).toEqual([
      ['ok', 1.9],
      ['errors', 0.1],
      ['previous', 1],
    ]);
    expect(series[2]?.dash).toEqual([4, 3]);
  });

  it('leaves a bucket without requests out of the error rate', () => {
    expect(errorRateSeries([point(0, 0, 0), point(1, 4, 1)], null, labels)[0]?.data).toEqual([null, 0.25]);
  });

  it('moves a compared trend onto the time in view', () => {
    expect(shiftTrend([point(10, 1, 0)], 5).map((entry) => entry.bucketAt)).toEqual([15]);
  });

  it('marks the versions first seen inside the range', () => {
    const versions = [
      { version: '1.0', firstSeenAt: 5 },
      { version: '2.0', firstSeenAt: 50 },
    ] as VersionSummary[];
    expect(deployAnnotations(versions, { from: 10, to: 100 }).map((entry) => entry.label)).toEqual(['2.0']);
  });
});
