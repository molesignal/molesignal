import { describe, expect, it } from 'vitest';

import {
  aggregateStats,
  analyzeMetricSeries,
  classifyColumns,
  compactMetricSeriesName,
  grafanaMetricSeriesName,
  rowsToSeries,
  seriesStats,
  topSeries,
} from '@/lib/metricsSeries';
import type { QueryResult } from '@/types/query';

function result(columns: string[], rows: unknown[][]): QueryResult {
  return { columns, rows, scanned_rows: rows.length, took_ms: 0 };
}

describe('classifyColumns', () => {
  it('treats _timestamp / time / ts as time columns', () => {
    const r = result(['_timestamp', 'value'], [[1, 2]]);
    expect(classifyColumns(r)).toEqual([
      { name: '_timestamp', kind: 'time' },
      { name: 'value', kind: 'value' },
    ]);
  });

  it('falls back to label for non-numeric columns', () => {
    const r = result(
      ['_timestamp', 'service', 'host', 'value'],
      [[1, 'foo', 'node-1', 42]],
    );
    expect(classifyColumns(r)).toEqual([
      { name: '_timestamp', kind: 'time' },
      { name: 'service', kind: 'label' },
      { name: 'host', kind: 'label' },
      { name: 'value', kind: 'value' },
    ]);
  });
});

describe('rowsToSeries', () => {
  it('returns empty for empty result', () => {
    expect(rowsToSeries(result(['_timestamp', 'value'], []))).toEqual([]);
  });

  it('produces a single series with empty labels when no label columns', () => {
    const series = rowsToSeries(
      result(
        ['_timestamp', 'value'],
        [
          [1_700_000_000_000_000, 1.0],
          [1_700_000_060_000_000, 2.0],
          [1_700_000_120_000_000, 3.0],
        ],
      ),
    );
    expect(series).toHaveLength(1);
    expect(series[0]!.labels).toEqual({});
    expect(series[0]!.values).toEqual([1.0, 2.0, 3.0]);
    expect(series[0]!.timestamps).toHaveLength(3);
    expect(series[0]!.valueColumn).toBe('value');
  });

  it('groups by label tuple and preserves first-seen order', () => {
    const series = rowsToSeries(
      result(
        ['_timestamp', 'service', 'host', 'value'],
        [
          [1, 'api', 'node-1', 10],
          [1, 'api', 'node-2', 20],
          [2, 'api', 'node-1', 11],
          [1, 'web', 'node-3', 30],
          [2, 'api', 'node-2', 21],
        ],
      ),
    );
    expect(series.map((s) => s.labels)).toEqual([
      { service: 'api', host: 'node-1' },
      { service: 'api', host: 'node-2' },
      { service: 'web', host: 'node-3' },
    ]);
    expect(series[0]!.values).toEqual([10, 11]);
    expect(series[1]!.values).toEqual([20, 21]);
    expect(series[2]!.values).toEqual([30]);
  });

  it('returns empty when no numeric value column found', () => {
    const series = rowsToSeries(
      result(['_timestamp', 'service'], [[1, 'api']]),
    );
    expect(series).toEqual([]);
  });

  it('coerces null label cells to dropped key, not "null"', () => {
    const series = rowsToSeries(
      result(
        ['service', 'value'],
        [
          ['api', 1],
          [null, 2],
        ],
      ),
    );
    expect(series.map((s) => s.labels)).toEqual([{ service: 'api' }, {}]);
  });
});

describe('topSeries', () => {
  it('picks the series with the highest last value by default', () => {
    const series = rowsToSeries(
      result(
        ['service', 'value'],
        [
          ['a', 1],
          ['a', 2],
          ['b', 5],
          ['b', 3],
        ],
      ),
    );
    // a values [1,2] last=2; b values [5,3] last=3 → b wins.
    expect(topSeries(series)?.labels).toEqual({ service: 'b' });
  });

  it('picks by max when requested', () => {
    const series = rowsToSeries(
      result(
        ['service', 'value'],
        [
          ['a', 100],
          ['a', 1],
          ['b', 5],
          ['b', 3],
        ],
      ),
    );
    expect(topSeries(series, 'max')?.labels).toEqual({ service: 'a' });
  });

  it('returns null on empty', () => {
    expect(topSeries([])).toBeNull();
  });
});

describe('stats', () => {
  it('per-series stats compute correctly', () => {
    const series = rowsToSeries(
      result(
        ['value'],
        [[10], [20], [30], [40], [50]],
      ),
    );
    const s = seriesStats(series[0]!);
    expect(s).not.toBeNull();
    expect(s!.min).toBe(10);
    expect(s!.max).toBe(50);
    expect(s!.last).toBe(50);
    expect(s!.avg).toBe(30);
  });

  it('aggregate stats pick the worst series', () => {
    const series = rowsToSeries(
      result(
        ['service', 'value'],
        [
          ['a', 100],
          ['a', 100],
          ['b', 1],
          ['b', 1],
        ],
      ),
    );
    const agg = aggregateStats(series);
    expect(agg!.p95).toBe(100); // worst series wins
    expect(agg!.min).toBe(1);   // best lower bound is min of series mins
  });

  it('returns null when no finite values', () => {
    expect(seriesStats({ labels: {}, values: [], timestamps: [], valueColumn: 'v' })).toBeNull();
    expect(aggregateStats([])).toBeNull();
  });
});

describe('series presentation and quality', () => {
  it('builds a concise service / method / route / status alias', () => {
    expect(
      compactMetricSeriesName({
        labels: {
          env: 'prod',
          service: 'checkout',
          method: 'GET',
          route: '/api/payments',
          status: '200',
          region: 'us-west-2',
        },
        values: [1],
        timestamps: [1],
        valueColumn: 'value',
      }),
    ).toBe('checkout · GET · /api/payments · 200');
  });

  it('formats a metric and its complete label set like Grafana Explore', () => {
    expect(
      grafanaMetricSeriesName(
        {
          labels: {
            'service.name': 'molesignal',
            pool: 'meta',
            node: 'querier "a"',
          },
          values: [16],
          timestamps: [1],
          valueColumn: 'value',
        },
        'db_pool_connections',
      ),
    ).toBe(
      'db_pool_connections{node="querier \\"a\\"", pool="meta", service.name="molesignal"}',
    );
  });

  it('keeps missing, negative, and timestamp anomalies distinct', () => {
    const quality = analyzeMetricSeries([
      {
        labels: { service: 'checkout' },
        values: [0, Number.NaN, -0.25, 1],
        timestamps: [1_000_000, 2_000_000, 2_000_000, 4_000_000],
        valueColumn: 'value',
      },
    ]);

    expect(quality.dataPoints).toBe(3);
    expect(quality.missingPoints).toBe(1);
    expect(quality.missingRatio).toBe(0.25);
    expect(quality.negativePoints).toBe(1);
    expect(quality.timestampAnomalies).toBe(1);
    expect(quality.estimatedStepSeconds).toBe(2);
  });
});
