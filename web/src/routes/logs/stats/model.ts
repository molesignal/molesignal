import { createDashboardPanel } from '@/dashboard-engine/factories';
import { createEmptyDashboardDefinition, dashboardDefinitionToModel } from '@/dashboard-engine/model';
import type { DashboardPanel, VisualizationType } from '@/dashboard-engine/schema';
import type { GlobalFilter } from '@/stores/useFiltersStore';
import type { TimeWindow } from '@/stores/useTimeStore';

import {
  escapeSqlIdentifier,
  escapeSqlString,
  logFieldQueryConditions,
  parseLogFieldStatement,
  type LogFieldDef,
} from '../fieldQueryModel';

/** What is worked out of the logs a query matches. All but a count are of a field. */
export const STATS_MEASURES = ['count', 'distinct', 'avg', 'sum', 'min', 'max', 'p50', 'p95', 'p99'] as const;
export type StatsMeasure = (typeof STATS_MEASURES)[number];

/** Grouped by nothing: one number for everything the query matches. */
export const STATS_GROUP_NONE = '';
/** Grouped by when the logs were written: the measure over time. */
export const STATS_GROUP_TIME = '@time';

export const STATS_LIMITS = [10, 20, 50, 100] as const;

export type StatsChart = 'stat' | 'bar' | 'line' | 'table';

/** What the statistics view works out, and how it is drawn. It is kept with the query tab. */
export interface LogStatsConfig {
  measure: StatsMeasure;
  /** The field that is measured. A count has none. */
  field: string;
  /** A field, {@link STATS_GROUP_TIME}, or {@link STATS_GROUP_NONE}. */
  groupBy: string;
  /** How many groups are kept, the largest first. */
  limit: number;
  chart: StatsChart;
}

/** Whether a measure is taken of numbers only: a count of different values is of anything. */
export function isNumericMeasure(measure: StatsMeasure): boolean {
  return measure !== 'count' && measure !== 'distinct';
}

/** The fields a measure can be taken of. */
export function measurableFields(fields: readonly LogFieldDef[], measure: StatsMeasure): LogFieldDef[] {
  if (measure === 'count') return [];

  return fields.filter((field) =>
    isNumericMeasure(measure)
      ? field.dataType === 'int64' || field.dataType === 'float64'
      : field.dataType !== 'json',
  );
}

/** The fields the logs can be grouped by: any but a document, whose values are whole documents. */
export function groupableFields(fields: readonly LogFieldDef[]): LogFieldDef[] {
  return fields.filter((field) => field.dataType !== 'json' && field.name !== '_timestamp');
}

/** Fields a reader is likely to want the logs counted by, in the order they are tried. */
const LIKELY_GROUPS = ['level', 'service.name', 'service', 'severity', 'status', 'http.status_code'];

/** What the view opens on: the logs counted by their level, or over time where they have none. */
export function defaultLogStatsConfig(fields: readonly LogFieldDef[]): LogStatsConfig {
  const names = new Set(groupableFields(fields).map((field) => field.name));
  const groupBy = LIKELY_GROUPS.find((name) => names.has(name)) ?? STATS_GROUP_TIME;

  return { measure: 'count', field: '', groupBy, limit: 20, chart: groupBy === STATS_GROUP_TIME ? 'line' : 'bar' };
}

/** The ways a result can be drawn, the first being the one it is drawn in unless another is asked for. */
export function statsChartsFor(groupBy: string): StatsChart[] {
  if (groupBy === STATS_GROUP_NONE) return ['stat', 'table'];

  return groupBy === STATS_GROUP_TIME ? ['line', 'bar', 'table'] : ['bar', 'table'];
}

/**
 * A config that can be run against `fields`: a measure keeps its field where
 * the stream has it and takes the first it can be taken of otherwise, a group
 * the stream no longer has falls back to time, and the chart is one the
 * grouping can be drawn in.
 */
export function settleLogStatsConfig(config: LogStatsConfig, fields: readonly LogFieldDef[]): LogStatsConfig {
  const measurable = measurableFields(fields, config.measure);
  const field = measurable.some((candidate) => candidate.name === config.field)
    ? config.field
    : measurable[0]?.name ?? '';
  // A measure with nothing to be taken of is a count.
  const measure: StatsMeasure = config.measure !== 'count' && field === '' ? 'count' : config.measure;
  const grouped = config.groupBy !== STATS_GROUP_NONE && config.groupBy !== STATS_GROUP_TIME;
  const groupBy =
    grouped && !groupableFields(fields).some((candidate) => candidate.name === config.groupBy)
      ? STATS_GROUP_TIME
      : config.groupBy;
  const charts = statsChartsFor(groupBy);

  return {
    measure,
    field: measure === 'count' ? '' : field,
    groupBy,
    limit: STATS_LIMITS.includes(config.limit as (typeof STATS_LIMITS)[number]) ? config.limit : 20,
    chart: charts.includes(config.chart) ? config.chart : charts[0]!,
  };
}

const SECOND = 1_000_000;
const TIME_STEPS = [1, 5, 10, 30, 60, 300, 600, 1_800, 3_600, 10_800, 21_600, 43_200, 86_400].map(
  (seconds) => seconds * SECOND,
);
/** About how many points a measure over time is drawn in. */
const TARGET_POINTS = 60;

/** The length of time one point of a measure over time stands for: a round one, few enough to read. */
export function statsBucketMicros(range: { start: number; end: number }): number {
  const span = Math.max(1, range.end - range.start);

  return TIME_STEPS.find((step) => span / step <= TARGET_POINTS) ?? TIME_STEPS.at(-1)!;
}

/**
 * The conditions the list selects by, as SQL: the field statement and the
 * pinned filters it does not already speak of. The statistics are of the logs
 * the list pages through.
 */
export function logStatsConditions(statement: string, globalFilters: readonly GlobalFilter[]): string[] {
  const named = new Set(parseLogFieldStatement(statement).filters.map((filter) => filter.field));
  const pinned = globalFilters
    .filter((filter) => filter.key && filter.value && !named.has(filter.key))
    .map(
      (filter) =>
        `"${escapeSqlIdentifier(filter.key)}" ${filter.operator === '!=' ? '!=' : '='} '${escapeSqlString(filter.value)}'`,
    );

  return [...logFieldQueryConditions(statement), ...pinned];
}

const PERCENTILE: Partial<Record<StatsMeasure, number>> = { p50: 0.5, p95: 0.95, p99: 0.99 };

/** The measure as SQL, and the name of its column in the result. */
export function statsMeasureSql(config: Pick<LogStatsConfig, 'measure' | 'field'>): { expression: string; alias: string } {
  if (config.measure === 'count') return { expression: 'COUNT(*)', alias: 'count' };

  const column = `"${escapeSqlIdentifier(config.field)}"`;
  const number = `CAST(${column} AS DOUBLE)`;
  // The column is named after what it holds, in words a table header and a legend can carry.
  const alias = `${config.measure}_${config.field.replace(/\W+/g, '_')}`;
  const share = PERCENTILE[config.measure];

  if (share !== undefined) {
    return { expression: `approx_percentile_cont(${share}) WITHIN GROUP (ORDER BY ${number})`, alias };
  }

  switch (config.measure) {
    case 'distinct':
      return { expression: `COUNT(DISTINCT ${column})`, alias };
    case 'avg':
      return { expression: `AVG(${number})`, alias };
    case 'sum':
      return { expression: `SUM(${number})`, alias };
    case 'min':
      return { expression: `MIN(${column})`, alias };
    default:
      return { expression: `MAX(${column})`, alias };
  }
}

interface LogStatsStatementInput {
  stream: string;
  /** The conditions the list selects by, as {@link logStatsConditions} gives them. */
  conditions: readonly string[];
  config: LogStatsConfig;
  /** What one point stands for when the measure is taken over time. */
  bucketMicros: number;
}

/**
 * The statement that works a config out. It names no time range: the range is
 * the one it is run over, so the same statement serves a dashboard whose reader
 * picks another.
 *
 * A group is read as text, so a number that is a code (a status) is a name on
 * an axis and not a quantity; and the column keeps the name of its field, so
 * it is masked where the field is.
 */
export function buildLogStatsStatement({ stream, conditions, config, bucketMicros }: LogStatsStatementInput): string {
  const { expression, alias } = statsMeasureSql(config);
  const measure = `${expression} AS "${alias}"`;
  const source = [
    `FROM "${escapeSqlIdentifier(stream)}"`,
    ...(conditions.length > 0 ? [`WHERE ${conditions.join('\n  AND ')}`] : []),
  ];

  if (config.groupBy === STATS_GROUP_NONE) return [`SELECT ${measure}`, ...source].join('\n');

  if (config.groupBy === STATS_GROUP_TIME) {
    // The start of the stretch a log falls in, in microseconds: a column named `time` is read as one.
    const bucket = `(CAST(_timestamp AS BIGINT) / ${bucketMicros}) * ${bucketMicros}`;

    return [`SELECT ${bucket} AS "time", ${measure}`, ...source, 'GROUP BY 1', 'ORDER BY 1'].join('\n');
  }

  const group = escapeSqlIdentifier(config.groupBy);

  return [
    `SELECT CAST("${group}" AS VARCHAR) AS "${group}", ${measure}`,
    ...source,
    'GROUP BY 1',
    'ORDER BY 2 DESC',
    `LIMIT ${config.limit}`,
  ].join('\n');
}

const VISUALIZATION: Record<StatsChart, VisualizationType> = {
  stat: 'stat',
  bar: 'bar_chart',
  line: 'time_series',
  table: 'table',
};

/** The colour a measure is drawn in. */
const STATS_COLOR = 'var(--chart-1)';

/** The most rows a statement of the reader's own is asked for, as the list asks. */
const STATEMENT_ROW_LIMIT = 1_000;

interface LogStatsPanelInput {
  title: string;
  stream: string;
  statement: string;
  chart: StatsChart;
  /** The result has a row for each stretch of time: bars over time are a time series drawn as bars. */
  overTime: boolean;
}

/**
 * The statistics as a dashboard panel: the statement over the stream, drawn the
 * way it is drawn here. The view renders this very panel, so what is saved to a
 * dashboard is what was on the screen.
 */
export function logStatsPanel({ title, stream, statement, chart, overTime }: LogStatsPanelInput): DashboardPanel {
  const timeBars = chart === 'bar' && overTime;
  const panel = createDashboardPanel([], timeBars ? 'time_series' : VISUALIZATION[chart]);

  return {
    ...panel,
    title,
    gridPos: { ...panel.gridPos, w: 24, h: 36 },
    queries: [
      {
        refId: 'A',
        enabled: true,
        dataSourceType: 'logs',
        query: { language: 'sql', statement, stream, streamType: 'logs', limit: STATEMENT_ROW_LIMIT },
      },
    ],
    visualization: {
      ...panel.visualization,
      options: {
        ...panel.visualization.options,
        ...(timeBars ? { drawStyle: 'bar', fillOpacity: 0.8 } : {}),
        // One series needs no table of its own under the chart.
        ...(chart === 'line' || timeBars ? { legendMode: 'list' } : {}),
      },
    },
    fieldConfig: {
      // One measure, drawn in the first colour of every chart of the product and not in one picked
      // by its name. Rows of a table are read, not drawn: their figures keep the colour of text.
      ...(chart === 'table' ? {} : { color: { mode: 'fixed' as const, value: STATS_COLOR } }),
      // Over time a measure is read against zero: a bar that begins at the smallest count says
      // nothing of how large a count is.
      ...(overTime ? { softMin: 0 } : {}),
    },
  };
}

/**
 * A new dashboard that holds the panel alone, as the backend stores it. Where
 * the explorer looks at a stretch of time that moves with the clock, the
 * dashboard opens on the same stretch.
 */
export function logStatsDashboardModel(title: string, panel: DashboardPanel, window: TimeWindow): Record<string, unknown> {
  const definition = createEmptyDashboardDefinition(title);

  return dashboardDefinitionToModel({
    ...definition,
    tags: ['logs'],
    timeSettings:
      window.mode === 'relative' && typeof window.from === 'string' && typeof window.to === 'string'
        ? { ...definition.timeSettings, defaultFrom: window.from, defaultTo: window.to }
        : definition.timeSettings,
    elements: [panel],
  });
}
