import { describe, expect, it } from 'vitest';

import type { LogFieldDef } from '../fieldQueryModel';
import { isResultOverTime, isStatisticsStatement, sampleStatisticsStatements, statsChartsForResult } from './written';

const field = (name: string): LogFieldDef => ({ name, dataType: 'utf8', type: 'string', count: '0', expandable: false });

describe('isStatisticsStatement', () => {
  it('tells a statement that works something out from one that lists rows', () => {
    expect(isStatisticsStatement('SELECT level, COUNT(*) FROM "app" GROUP BY level')).toBe(true);
    expect(isStatisticsStatement('select avg(duration_ms) from "app"')).toBe(true);
    expect(isStatisticsStatement('SELECT approx_percentile_cont(0.9) WITHIN GROUP (ORDER BY d) FROM "app"')).toBe(true);
    expect(isStatisticsStatement('SELECT * FROM "app" WHERE level = \'ERROR\' ORDER BY _timestamp DESC LIMIT 100')).toBe(false);
    // A column that is named like a measure is not one.
    expect(isStatisticsStatement('SELECT count, max_size FROM "app"')).toBe(false);
  });
});

describe('statsChartsForResult', () => {
  it('draws a measure over time as lines first', () => {
    const result = { columns: ['time', 'count'], rows: [[1, 2], [2, 3]] };

    expect(statsChartsForResult(result)).toEqual(['line', 'bar', 'table']);
    expect(isResultOverTime(result)).toBe(true);
  });

  it('draws a measure by name as bars first', () => {
    const result = { columns: ['level', 'count'], rows: [['INFO', 2], ['ERROR', 3]] };

    expect(statsChartsForResult(result)).toEqual(['bar', 'table']);
    expect(isResultOverTime(result)).toBe(false);
  });

  it('draws one figure as one figure, and rows without a number as a table', () => {
    expect(statsChartsForResult({ columns: ['count'], rows: [[354]] })).toEqual(['stat', 'table']);
    expect(statsChartsForResult({ columns: ['a', 'b'], rows: [[1, 2], [3, 4]] })).toEqual(['table', 'bar']);
    expect(statsChartsForResult({ columns: ['level'], rows: [['INFO']] })).toEqual(['table']);
  });
});

describe('sampleStatisticsStatements', () => {
  it('offers the logs counted by what tells them apart, and over time', () => {
    const samples = sampleStatisticsStatements('app_logs', [field('level'), field('message')], { start: 0, end: 3_600_000_000 });

    expect(samples).toHaveLength(2);
    expect(samples[0]).toContain('CAST("level" AS VARCHAR) AS "level", COUNT(*) AS "count"');
    expect(samples[1]).toContain('(CAST(_timestamp AS BIGINT) / 60000000) * 60000000 AS "time"');
    expect(samples.every(isStatisticsStatement)).toBe(true);
  });

  it('offers the one over time where nothing tells the logs apart', () => {
    expect(sampleStatisticsStatements('app_logs', [field('message')], null)).toHaveLength(1);
  });
});
