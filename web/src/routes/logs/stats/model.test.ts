import { describe, expect, it } from 'vitest';

import { validateDashboardDefinition } from '@/dashboard-engine/model';
import type { DashboardDefinition, DashboardPanel } from '@/dashboard-engine/schema';

import type { LogFieldDef } from '../fieldQueryModel';
import {
  buildLogStatsStatement,
  defaultLogStatsConfig,
  groupableFields,
  logStatsConditions,
  logStatsDashboardModel,
  logStatsPanel,
  measurableFields,
  settleLogStatsConfig,
  STATS_GROUP_NONE,
  STATS_GROUP_TIME,
  statsBucketMicros,
  statsChartsFor,
  type LogStatsConfig,
} from './model';

const field = (name: string, dataType: LogFieldDef['dataType'] = 'utf8'): LogFieldDef => ({
  name,
  dataType,
  type: dataType === 'int64' || dataType === 'float64' ? 'number' : 'string',
  count: '0',
  expandable: false,
});
const FIELDS = [
  field('_timestamp', 'timestamp'),
  field('level'),
  field('service.name'),
  field('duration_ms', 'float64'),
  field('http.status_code', 'int64'),
  field('payload', 'json'),
];
const config = (change: Partial<LogStatsConfig> = {}): LogStatsConfig => ({
  measure: 'count',
  field: '',
  groupBy: 'level',
  limit: 20,
  chart: 'bar',
  ...change,
});
const MINUTE = 60_000_000;
const statement = (change: Partial<LogStatsConfig>, conditions: string[] = []) =>
  buildLogStatsStatement({ stream: 'app_logs', conditions, config: config(change), bucketMicros: 5 * MINUTE });

describe('what a log stream can be measured and grouped by', () => {
  it('takes numbers for a measure of numbers, and anything but a document for a count of different values', () => {
    expect(measurableFields(FIELDS, 'count')).toEqual([]);
    expect(measurableFields(FIELDS, 'avg').map((item) => item.name)).toEqual(['duration_ms', 'http.status_code']);
    expect(measurableFields(FIELDS, 'distinct').map((item) => item.name)).not.toContain('payload');
  });

  it('groups by any field but a document, and by time through its own choice', () => {
    expect(groupableFields(FIELDS).map((item) => item.name)).toEqual([
      'level',
      'service.name',
      'duration_ms',
      'http.status_code',
    ]);
  });

  it('opens on the logs counted by their level, or over time where they have none', () => {
    expect(defaultLogStatsConfig(FIELDS)).toMatchObject({ measure: 'count', groupBy: 'level', chart: 'bar' });
    expect(defaultLogStatsConfig([field('message')])).toMatchObject({ groupBy: STATS_GROUP_TIME, chart: 'line' });
  });
});

describe('settleLogStatsConfig', () => {
  it('gives a measure the first field it can be taken of', () => {
    expect(settleLogStatsConfig(config({ measure: 'p95' }), FIELDS)).toMatchObject({
      measure: 'p95',
      field: 'duration_ms',
    });
  });

  it('falls back to a count where the stream has nothing to measure', () => {
    expect(settleLogStatsConfig(config({ measure: 'avg', field: 'gone' }), [field('level')])).toMatchObject({
      measure: 'count',
      field: '',
    });
  });

  it('groups over time where the field it grouped by is gone, and draws it a way that can be', () => {
    expect(settleLogStatsConfig(config({ groupBy: 'gone', chart: 'stat' }), FIELDS)).toMatchObject({
      groupBy: STATS_GROUP_TIME,
      chart: 'line',
    });
    expect(settleLogStatsConfig(config({ groupBy: STATS_GROUP_NONE, chart: 'bar' }), FIELDS).chart).toBe('stat');
    expect(settleLogStatsConfig(config({ limit: 7 }), FIELDS).limit).toBe(20);
  });

  it('offers the charts a grouping can be drawn in', () => {
    expect(statsChartsFor(STATS_GROUP_NONE)).toEqual(['stat', 'table']);
    expect(statsChartsFor(STATS_GROUP_TIME)).toEqual(['line', 'bar', 'table']);
    expect(statsChartsFor('level')).toEqual(['bar', 'table']);
  });
});

describe('statsBucketMicros', () => {
  it('is a round length that keeps a measure over time to about sixty points', () => {
    expect(statsBucketMicros({ start: 0, end: 60 * MINUTE })).toBe(MINUTE);
    expect(statsBucketMicros({ start: 0, end: 24 * 60 * MINUTE })).toBe(30 * MINUTE);
    expect(statsBucketMicros({ start: 0, end: 30 * 24 * 60 * MINUTE })).toBe(12 * 60 * MINUTE);
    expect(statsBucketMicros({ start: 0, end: 400 * 24 * 60 * MINUTE })).toBe(24 * 60 * MINUTE);
  });
});

describe('logStatsConditions', () => {
  it('selects by the field statement and the pinned filters it does not already speak of', () => {
    expect(
      logStatsConditions("level = 'ERROR' AND timeout", [
        { key: 'level', operator: '=', value: 'WARN' },
        { key: 'env', operator: '!=', value: "o'brien" },
        { key: '', operator: '=', value: 'x' },
      ] as never),
    ).toEqual([`"level" = 'ERROR'`, `"message" LIKE '%timeout%'`, `"env" != 'o''brien'`]);
  });

  it('selects everything for an empty statement', () => {
    expect(logStatsConditions('', [])).toEqual([]);
  });
});

describe('buildLogStatsStatement', () => {
  it('counts the logs by a field read as text, the largest groups first', () => {
    expect(statement({}, [`"env" = 'prod'`, `"level" != ''`])).toBe(
      [
        'SELECT CAST("level" AS VARCHAR) AS "level", COUNT(*) AS "count"',
        'FROM "app_logs"',
        `WHERE "env" = 'prod'`,
        `  AND "level" != ''`,
        'GROUP BY 1',
        'ORDER BY 2 DESC',
        'LIMIT 20',
      ].join('\n'),
    );
  });

  it('takes a measure over stretches of time that begin on a multiple of their length', () => {
    expect(statement({ measure: 'p95', field: 'duration_ms', groupBy: STATS_GROUP_TIME })).toBe(
      [
        'SELECT (CAST(_timestamp AS BIGINT) / 300000000) * 300000000 AS "time", approx_percentile_cont(0.95) WITHIN GROUP (ORDER BY CAST("duration_ms" AS DOUBLE)) AS "p95_duration_ms"',
        'FROM "app_logs"',
        'GROUP BY 1',
        'ORDER BY 1',
      ].join('\n'),
    );
  });

  it('gives one figure where nothing groups the logs', () => {
    expect(statement({ measure: 'distinct', field: 'service.name', groupBy: STATS_GROUP_NONE })).toBe(
      'SELECT COUNT(DISTINCT "service.name") AS "distinct_service_name"\nFROM "app_logs"',
    );
  });

  it('writes names that hold a quote so that they stay names', () => {
    const written = buildLogStatsStatement({
      stream: 'app"logs',
      conditions: [],
      config: config({ measure: 'max', field: 'a"b', groupBy: 'c"d' }),
      bucketMicros: MINUTE,
    });

    expect(written).toContain('FROM "app""logs"');
    expect(written).toContain('MAX("a""b") AS "max_a_b"');
    expect(written).toContain('CAST("c""d" AS VARCHAR) AS "c""d"');
  });

  it.each([
    ['avg', 'AVG(CAST("duration_ms" AS DOUBLE))'],
    ['sum', 'SUM(CAST("duration_ms" AS DOUBLE))'],
    ['min', 'MIN("duration_ms")'],
    ['p50', 'approx_percentile_cont(0.5) WITHIN GROUP (ORDER BY CAST("duration_ms" AS DOUBLE))'],
    ['p99', 'approx_percentile_cont(0.99) WITHIN GROUP (ORDER BY CAST("duration_ms" AS DOUBLE))'],
  ] as const)('writes %s as SQL', (measure, expression) => {
    expect(statement({ measure, field: 'duration_ms' })).toContain(`${expression} AS "${measure}_duration_ms"`);
  });
});

describe('the statistics as a dashboard', () => {
  const panel = (change: Partial<Parameters<typeof logStatsPanel>[0]> = {}): DashboardPanel =>
    logStatsPanel({ title: 'Count by level', stream: 'app_logs', statement: 'SELECT 1', chart: 'bar', overTime: false, ...change });

  it('is a panel that runs the statement over the log stream', () => {
    expect(panel().queries).toEqual([
      {
        refId: 'A',
        enabled: true,
        dataSourceType: 'logs',
        query: { language: 'sql', statement: 'SELECT 1', stream: 'app_logs', streamType: 'logs', limit: 1_000 },
      },
    ]);
    expect(panel().visualization.type).toBe('bar_chart');
    expect(panel({ chart: 'table' }).visualization.type).toBe('table');
    expect(panel({ chart: 'stat' }).visualization.type).toBe('stat');
  });

  it('draws a measure over time as a time series read against zero, in lines or in bars', () => {
    const line = panel({ chart: 'line', overTime: true });
    const bars = panel({ chart: 'bar', overTime: true });

    expect(line.visualization).toMatchObject({ type: 'time_series', options: { drawStyle: 'line' } });
    expect(bars.visualization).toMatchObject({ type: 'time_series', options: { drawStyle: 'bar' } });
    expect(line.fieldConfig).toMatchObject({ softMin: 0 });
    expect(panel().fieldConfig).toEqual({ color: { mode: 'fixed', value: 'var(--chart-1)' } });
    // The figures of a table are text: they are not coloured as a mark of a chart is.
    expect(panel({ chart: 'table' }).fieldConfig).toEqual({});
  });

  it('is saved as a dashboard the engine takes, opening on the stretch of time the explorer looks at', () => {
    const model = logStatsDashboardModel('Checkout logs', panel(), { mode: 'relative', from: 'now-24h', to: 'now' });

    expect(validateDashboardDefinition(model as unknown as DashboardDefinition)).toEqual({ valid: true, issues: [] });
    expect(model).toMatchObject({
      title: 'Checkout logs',
      tags: ['logs'],
      timeSettings: { defaultFrom: 'now-24h', defaultTo: 'now' },
      elements: [{ kind: 'panel', title: 'Count by level', gridPos: { w: 24 } }],
    });
  });

  it('keeps the dashboard on its own stretch of time where the explorer looks at fixed moments', () => {
    const model = logStatsDashboardModel('Checkout logs', panel(), {
      mode: 'absolute',
      from: '2026-05-23T09:00:00.000Z',
      to: '2026-05-23T10:00:00.000Z',
    });

    expect(model).toMatchObject({ timeSettings: { defaultFrom: 'now-6h', defaultTo: 'now' } });
  });
});
