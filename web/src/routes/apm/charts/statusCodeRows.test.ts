import { describe, expect, it } from 'vitest';

import type { RedSummary, TrendPoint } from '@/api/apm';

import { statusClassSeries, statusOf, statusRows } from './statusCodeRows';

const red = (status_codes: Record<string, number>): RedSummary => ({
  requestCount: Object.values(status_codes).reduce((sum, count) => sum + count, 0),
  errorCount: 0,
  errorRate: 0,
  durationSum: 0,
  latencyPartial: false,
  exemplars: [],
  statusCodes: status_codes,
});

describe('status codes', () => {
  it('reads HTTP and gRPC codes, and the rest of a protocol', () => {
    expect(statusOf('http:404')).toMatchObject({ protocol: 'http', label: '404', statusClass: '4xx' });
    expect(statusOf('grpc:14')).toMatchObject({ protocol: 'grpc', label: 'UNAVAILABLE', code: 14, statusClass: 'grpc_error' });
    expect(statusOf('grpc:0')).toMatchObject({ label: 'OK', statusClass: 'grpc_ok' });
    expect(statusOf('http:other')).toMatchObject({ protocol: 'http', label: '', statusClass: 'other' });
  });

  it('lists the statuses the commonest first, each with its share', () => {
    const rows = statusRows({ 'http:200': 90, 'http:503': 6, 'http:404': 4 });
    expect(rows.map((row) => [row.label, row.count, row.share])).toEqual([
      ['200', 90, 0.9],
      ['503', 6, 0.06],
      ['404', 4, 0.04],
    ]);
    expect(statusRows(undefined)).toEqual([]);
  });

  it('stacks the classes a second from the healthy up, only those there are', () => {
    const trend: TrendPoint[] = [
      { bucketAt: 0, red: red({ 'http:200': 60, 'http:500': 6 }) },
      { bucketAt: 60, red: red({ 'http:200': 120 }) },
    ];
    const series = statusClassSeries(trend, 60, (statusClass) => statusClass);

    expect(series.map((entry) => [entry.id, entry.data])).toEqual([
      ['2xx', [1, 2]],
      ['5xx', [0.1, 0]],
    ]);
  });
});
