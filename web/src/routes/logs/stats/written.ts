import { inferFieldType } from '@/dashboard-engine/dataframe';
import type { QueryResult } from '@/types/query';

import {
  buildLogStatsStatement,
  defaultLogStatsConfig,
  STATS_GROUP_TIME,
  statsBucketMicros,
  type LogStatsConfig,
  type StatsChart,
} from './model';
import type { LogFieldDef } from '../fieldQueryModel';

/**
 * Whether a statement the reader wrote works something out of the logs, as
 * opposed to listing them. A list of rows has no chart: a line through the
 * status codes of a hundred logs says nothing.
 */
export function isStatisticsStatement(statement: string): boolean {
  return (
    /\bGROUP\s+BY\b/i.test(statement) ||
    /\b(COUNT|SUM|AVG|MIN|MAX|MEDIAN|STDDEV\w*|VAR\w*|APPROX_\w+|PERCENTILE_\w+)\s*\(/i.test(statement)
  );
}

/**
 * The ways the result of a written statement can be drawn, the first being the
 * one it is drawn in unless another is asked for: over time where it has a
 * time, in bars where it has names, as one figure where it is one.
 */
export function statsChartsForResult(result: Pick<QueryResult, 'columns' | 'rows'>): StatsChart[] {
  const types = result.columns.map((name, index) =>
    inferFieldType(
      name,
      result.rows.map((row) => row[index]),
    ),
  );

  if (!types.includes('number')) return ['table'];
  if (types.includes('time')) return ['line', 'bar', 'table'];
  if (types.includes('string') || types.includes('boolean')) return ['bar', 'table'];

  return result.rows.length === 1 ? ['stat', 'table'] : ['table', 'bar'];
}

/** Whether a result is a measure over time, which decides how its bars are drawn. */
export function isResultOverTime(result: Pick<QueryResult, 'columns' | 'rows'>): boolean {
  return result.columns.some(
    (name, index) =>
      inferFieldType(
        name,
        result.rows.map((row) => row[index]),
      ) === 'time',
  );
}

/**
 * Statements to begin from, for a reader whose statement lists rows: the logs
 * counted by what the stream is most likely told apart by, and over time.
 */
export function sampleStatisticsStatements(
  stream: string,
  fields: readonly LogFieldDef[],
  range: { start: number; end: number } | null,
): string[] {
  const grouped = defaultLogStatsConfig(fields);
  const overTime: LogStatsConfig = { ...grouped, groupBy: STATS_GROUP_TIME, chart: 'line' };
  const bucketMicros = statsBucketMicros(range ?? { start: 0, end: 3_600_000_000 });
  const statement = (config: LogStatsConfig) => buildLogStatsStatement({ stream, conditions: [], config, bucketMicros });

  return grouped.groupBy === STATS_GROUP_TIME ? [statement(overTime)] : [statement(grouped), statement(overTime)];
}
