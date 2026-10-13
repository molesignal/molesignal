import { useQuery } from '@tanstack/react-query';
import * as React from 'react';

import * as queryApi from '@/api/query';
import type { QueryResult } from '@/types/query';

import type { TraceFieldDef } from '../fieldQueryModel';
import {
  hasTraceLevelValues,
  NO_TRACE_FIELD_VALUES,
  TRACE_FIELD_VALUE_LIMIT,
  traceLevelFieldValues,
  type TraceFieldRecord,
  type TraceFieldValues,
} from './model';

/** Where a field's values are read from: the stream the traces are of, and the range they were asked from. */
export interface TraceFieldValueSource {
  orgId: string;
  stream: string;
  /** Microseconds since the epoch. */
  range: { start: number; end: number };
}

const identifier = (name: string) => `"${name.replace(/"/g, '""')}"`;
const literal = (text: string) => `'${text.replace(/'/g, "''")}'`;

/**
 * The two statements that say what the loaded traces hold for a field of
 * their spans: its most common values, each with the traces and spans it is
 * in, and what they add up to. Both read only the spans of the loaded traces.
 *
 * Everything comes back as text. A count of nanoseconds since the epoch is
 * past what a JSON number holds exactly, and a filter on it has to be exact.
 */
export function traceFieldValueStatements(
  stream: string,
  field: Pick<TraceFieldDef, 'name' | 'dataType'>,
  traceIds: readonly string[],
): { top: string; summary: string } {
  const column = identifier(field.name);
  // A timestamp is filtered as the number it is stored as, so that is how it is read.
  const stored = field.dataType === 'timestamp' ? `CAST(${column} AS BIGINT)` : column;
  const text = `CAST(${stored} AS VARCHAR)`;
  // A span without the field, or with an empty one, has no value to list or to filter by.
  const scope = `FROM ${identifier(stream)} WHERE trace_id IN (${traceIds.map(literal).join(', ')}) AND ${column} IS NOT NULL AND ${text} <> ''`;
  const whole = field.dataType === 'int64' || field.dataType === 'timestamp';
  const average = whole ? `CAST(AVG(${stored}) AS BIGINT)` : `AVG(${stored})`;
  const range = whole || field.dataType === 'float64'
    ? `, CAST(MIN(${stored}) AS VARCHAR) AS min_value, CAST(MAX(${stored}) AS VARCHAR) AS max_value, CAST(${average} AS VARCHAR) AS avg_value`
    : '';

  return {
    top: `SELECT ${text} AS value, COUNT(DISTINCT trace_id) AS traces, COUNT(*) AS spans ${scope} GROUP BY 1 ORDER BY traces DESC, spans DESC, value ASC LIMIT ${TRACE_FIELD_VALUE_LIMIT}`,
    summary: `SELECT COUNT(DISTINCT ${text}) AS distinct_values, COUNT(DISTINCT trace_id) AS traces, COUNT(*) AS samples${range} ${scope}`,
  };
}

/**
 * The stretch of time the loaded traces cover, a second to spare at either
 * end: the query reads those minutes and not the whole range that was asked
 * for. `null` when the traces carry no usable start.
 */
export function loadedTraceWindow(
  traces: readonly TraceFieldRecord[],
): { start: number; end: number } | null {
  const timed = traces.filter((trace) => Number.isFinite(trace.startNs) && trace.startNs > 0);
  if (timed.length === 0) return null;

  const starts = timed.map((trace) => Math.floor(trace.startNs / 1_000));
  const ends = timed.map((trace) =>
    Math.ceil(trace.startNs / 1_000 + Math.max(0, Number.isFinite(trace.durationMs) ? trace.durationMs : 0) * 1_000),
  );

  return { start: Math.min(...starts) - 1_000_000, end: Math.max(...ends) + 1_000_000 };
}

const cell = (result: QueryResult, row: unknown[] | undefined, column: string): unknown =>
  row?.[result.columns.indexOf(column)];
const count = (value: unknown): number => (Number.isFinite(Number(value)) ? Number(value) : 0);
const written = (value: unknown): string => (value === null || value === undefined ? '' : String(value));

/** The answers to the two statements, as the panel shows them. */
export function traceFieldValuesFrom(top: QueryResult, summary: QueryResult): TraceFieldValues {
  const totals = summary.rows[0];
  const [min, max, avg] = ['min_value', 'max_value', 'avg_value'].map((column) => written(cell(summary, totals, column)));

  return {
    traces: count(cell(summary, totals, 'traces')),
    distinct: count(cell(summary, totals, 'distinct_values')),
    samples: count(cell(summary, totals, 'samples')),
    top: top.rows
      .map((row) => ({
        value: written(cell(top, row, 'value')),
        traces: count(cell(top, row, 'traces')),
        spans: count(cell(top, row, 'spans')),
      }))
      .filter((item) => item.value !== ''),
    ...(min && max && avg && { range: { min, max, avg } }),
  };
}

export interface TraceFieldValuesState {
  values: TraceFieldValues | undefined;
  loading: boolean;
  failed: boolean;
  retry: () => void;
}

/**
 * What the loaded traces hold for a field, read when the field is opened.
 *
 * The list loads one row per trace, which says nothing of span ids, kinds or
 * attributes. So a field of a span is read from the spans of the loaded
 * traces themselves: the values are real, and they are of exactly the traces
 * on the page. A field a trace has one value for is read from the rows.
 */
export function useTraceFieldValues(
  field: TraceFieldDef,
  traces: readonly TraceFieldRecord[],
  source: TraceFieldValueSource,
): TraceFieldValuesState {
  const local = hasTraceLevelValues(field);
  const ids = React.useMemo(() => traces.map((trace) => trace.id).filter(Boolean), [traces]);
  const onRows = React.useMemo(
    () => (local ? traceLevelFieldValues(traces, field.name) : undefined),
    [field.name, local, traces],
  );
  const asked = !local && ids.length > 0 && Boolean(source.orgId && source.stream);

  const query = useQuery({
    queryKey: ['trace-field-values', source.orgId, source.stream, field.name, field.dataType, ids],
    queryFn: async (): Promise<TraceFieldValues> => {
      const statements = traceFieldValueStatements(source.stream, field, ids);
      const run = (statement: string, limit: number) =>
        queryApi.runQuery({
          org_id: source.orgId,
          language: 'sql',
          statement,
          time_range: loadedTraceWindow(traces) ?? source.range,
          stream: { name: source.stream, stream_type: 'traces' },
          limit,
        });
      const [top, summary] = await Promise.all([run(statements.top, TRACE_FIELD_VALUE_LIMIT), run(statements.summary, 1)]);

      return traceFieldValuesFrom(top, summary);
    },
    enabled: asked,
    staleTime: 60_000,
  });

  if (local) return { values: onRows, loading: false, failed: false, retry: () => undefined };
  // No traces, or no stream to read them from: there is nothing to ask, and nothing to wait for.
  if (!asked) return { values: NO_TRACE_FIELD_VALUES, loading: false, failed: false, retry: () => undefined };

  return {
    values: query.data,
    loading: query.isLoading,
    failed: query.isError,
    retry: () => void query.refetch(),
  };
}
