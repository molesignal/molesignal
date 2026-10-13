import { describe, expect, it } from 'vitest';

import type { TraceFieldRecord } from './model';
import { loadedTraceWindow, traceFieldValuesFrom, traceFieldValueStatements } from './valuesQuery';

const trace = (id: string, startNs: number, durationMs: number): TraceFieldRecord => ({
  id,
  op: 'GET /',
  service: 'web',
  startNs,
  durationMs,
  spans: 1,
  errors: 0,
});

describe('the statements that read a field of the loaded spans', () => {
  it('count each value in traces and spans, among the loaded traces only', () => {
    const { top, summary } = traceFieldValueStatements('otel"x', { name: 'service.name', dataType: 'utf8' }, ['a', "b'c"]);

    expect(top).toBe(
      'SELECT CAST("service.name" AS VARCHAR) AS value, COUNT(DISTINCT trace_id) AS traces, COUNT(*) AS spans '
      + `FROM "otel""x" WHERE trace_id IN ('a', 'b''c') AND "service.name" IS NOT NULL AND CAST("service.name" AS VARCHAR) <> '' `
      + 'GROUP BY 1 ORDER BY traces DESC, spans DESC, value ASC LIMIT 8',
    );
    expect(summary).toContain('COUNT(DISTINCT CAST("service.name" AS VARCHAR)) AS distinct_values');
    expect(summary).toContain('COUNT(DISTINCT trace_id) AS traces, COUNT(*) AS samples FROM');
    expect(summary).not.toContain('min_value');
  });

  it('ask a number for its range as text, so that nanoseconds are not rounded', () => {
    const { summary } = traceFieldValueStatements('default', { name: 'start_time_unix_nano', dataType: 'int64' }, ['a']);

    expect(summary).toContain('CAST(MIN("start_time_unix_nano") AS VARCHAR) AS min_value');
    expect(summary).toContain('CAST(CAST(AVG("start_time_unix_nano") AS BIGINT) AS VARCHAR) AS avg_value');
  });

  it('read a timestamp as the number a filter compares it with', () => {
    const { top, summary } = traceFieldValueStatements('default', { name: 'seen_at', dataType: 'timestamp' }, ['a']);

    expect(top).toContain('SELECT CAST(CAST("seen_at" AS BIGINT) AS VARCHAR) AS value');
    expect(summary).toContain('CAST(MAX(CAST("seen_at" AS BIGINT)) AS VARCHAR) AS max_value');
  });
});

describe('the stretch of time the loaded traces cover', () => {
  it('runs from the earliest start to the latest end, a second to spare', () => {
    expect(loadedTraceWindow([
      trace('a', 2_000_000_000_000, 500),
      trace('b', 1_000_000_000_000, 250),
    ])).toEqual({ start: 1_000_000_000 - 1_000_000, end: 2_000_500_000 + 1_000_000 });
  });

  it('is unknown for traces that carry no start', () => {
    expect(loadedTraceWindow([trace('a', 0, 10)])).toBeNull();
    expect(loadedTraceWindow([])).toBeNull();
  });
});

describe('the answers', () => {
  const top = { columns: ['value', 'traces', 'spans'], rows: [['web', 19, 40], ['router', 1, 2], [null, 1, 1]], scanned_rows: 0, took_ms: 0 };

  it('become the values with what they add up to', () => {
    const summary = { columns: ['distinct_values', 'traces', 'samples'], rows: [[2, 20, 43]], scanned_rows: 0, took_ms: 0 };

    expect(traceFieldValuesFrom(top, summary)).toEqual({
      traces: 20,
      distinct: 2,
      samples: 43,
      top: [{ value: 'web', traces: 19, spans: 40 }, { value: 'router', traces: 1, spans: 2 }],
    });
  });

  it('keep a range as it was written', () => {
    const summary = {
      columns: ['distinct_values', 'traces', 'samples', 'min_value', 'max_value', 'avg_value'],
      rows: [[55, 20, 55, '1791701310072074000', '1791701335959397000', '1791701320000000000']],
      scanned_rows: 0,
      took_ms: 0,
    };

    expect(traceFieldValuesFrom(top, summary).range).toEqual({
      min: '1791701310072074000',
      max: '1791701335959397000',
      avg: '1791701320000000000',
    });
  });

  it('have no range when no span had a value', () => {
    const summary = {
      columns: ['distinct_values', 'traces', 'samples', 'min_value', 'max_value', 'avg_value'],
      rows: [[0, 0, 0, null, null, null]],
      scanned_rows: 0,
      took_ms: 0,
    };
    const none = { ...top, rows: [] };

    expect(traceFieldValuesFrom(none, summary)).toEqual({ traces: 0, distinct: 0, samples: 0, top: [] });
  });
});
