import type { RumAggregate, RumAggregateParams, RumFilter, RumMeasure, RumRow, RumSearchParams } from '@/api/rum';
import type { TimeSeriesThreshold } from '@/viz/timeseries/types';

import {
  count,
  type Graded,
  gradeOf,
  type Grade,
  inUnit,
  named,
  numeric,
  type OverviewRange,
  type OverviewScope,
  percentile,
  scopeFilters,
  VITALS,
  type VitalKey,
} from '../overview/model';

// ---------------------------------------------------------------------------------------------
// What a view is measured by: the three Core Web Vitals, and two levels read beside them.

export type MetricKey = VitalKey | 'fcp' | 'ttfb';

export interface Metric extends Graded {
  key: MetricKey;
  /** The column of a view that holds it. */
  field: string;
  /** One of the three Core Web Vitals; the others are read beside them and have no breakdown. */
  core: boolean;
}

const FCP: Metric = { key: 'fcp', field: 'view.performance.fcp.timestamp', nanoseconds: true, good: 1_800, poor: 3_000, scaleMax: 4_500, core: false };
const TTFB: Metric = { key: 'ttfb', field: 'view.first_byte', nanoseconds: true, good: 800, poor: 1_800, scaleMax: 2_700, core: false };

/** In the order the page tells them: the vitals, then the two that come before them. */
export const METRICS: readonly Metric[] = [...VITALS.map((vital) => ({ ...vital, core: true })), FCP, TTFB];
export const CORE_METRICS = METRICS.filter((metric) => metric.core);
export const SUPPORT_METRICS = METRICS.filter((metric) => !metric.core);

export const DEFAULT_METRIC: MetricKey = 'lcp';

export const metricOf = (key: MetricKey): Metric => METRICS.find((metric) => metric.key === key) as Metric;

export const parseMetric = (value: string | null): MetricKey => METRICS.find((metric) => metric.key === value)?.key ?? DEFAULT_METRIC;

/** A level in the unit the SDK stores it in, as a filter writes it. */
export const rawOf = (metric: Pick<Graded, 'nanoseconds'>, value: number): string =>
  String(metric.nanoseconds ? Math.round(value * 1_000_000) : value);

// ---------------------------------------------------------------------------------------------
// What the views are told apart by.

export const DIMENSIONS = [
  { key: 'page', field: 'view.url_path_group' },
  { key: 'browser', field: 'browser.name' },
  { key: 'os', field: 'os.name' },
  { key: 'device', field: 'device.type' },
  { key: 'country', field: 'geo.country_iso_code' },
  { key: 'version', field: 'version' },
] as const;

export type Dimension = (typeof DIMENSIONS)[number];
export type DimensionKey = Dimension['key'];

export const DEFAULT_DIMENSION: DimensionKey = 'page';

export const dimensionOf = (key: DimensionKey): Dimension => DIMENSIONS.find((dimension) => dimension.key === key) as Dimension;

export const parseDimension = (value: string | null): DimensionKey =>
  DIMENSIONS.find((dimension) => dimension.key === value)?.key ?? DEFAULT_DIMENSION;

/** The rows the table has at the most: the values with the most views; a reader orders the ones that are there. */
export const TABLE_ROWS = 50;

/** A value with fewer views than this is shown, but a ranking by a level leaves it to the end: a few views say nothing. */
export const MIN_VIEWS = 30;

// ---------------------------------------------------------------------------------------------
// What the page asks.

const view = (scope: OverviewScope, range: OverviewRange, extra: RumFilter[] = []) => ({
  type: 'views' as const,
  from: range.from,
  to: range.to,
  filters: scopeFilters(scope, extra),
});

const exists = (field: string): RumFilter => ({ field, op: 'exists', value: '' });

const within = (metric: Metric, comparison: '<=' | '>', level: number): RumFilter => ({
  field: metric.field,
  op: comparison,
  value: rawOf(metric, level),
});

/** How many views had a value of a level: a count of the column counts the rows that have one. */
const samples = (name: string, field: string): RumMeasure => ({ fn: 'count', field, name });

/**
 * How many views there were and, of each level, its 75th percentile, how many views had it, and how
 * many of those were good and how many were poor; the ones in between are what is left.
 */
export function levelsQuery(scope: OverviewScope, range: OverviewRange, metrics: readonly Metric[]): RumAggregateParams {
  return {
    ...view(scope, range),
    measures: [
      count('views'),
      ...metrics.flatMap((metric) => [
        percentile(metric.key, metric.field),
        samples(`${metric.key}_samples`, metric.field),
        count(`${metric.key}_good`, [within(metric, '<=', metric.good)]),
        count(`${metric.key}_poor`, [within(metric, '>', metric.poor)]),
      ]),
    ],
  };
}

/** The 75th percentile of a level in each bar of the range; the bars begin where the frame does. */
export function trendQuery(
  scope: OverviewScope,
  bars: { binMicros: number; frame: { start: number; end: number } },
  metric: Metric,
  extra: RumFilter[] = [],
): RumAggregateParams {
  return {
    ...view(scope, { from: bars.frame.start, to: bars.frame.end }, extra),
    measures: [percentile('p75', metric.field)],
    bucketTime: bars.binMicros,
    originTime: bars.frame.start,
  };
}

/**
 * The most viewed values of a dimension, each with the 75th percentile of every level and, of every
 * level, how many of its views had it and how many of those were good. The read does not depend on
 * the level in front, so picking another is not a new read.
 */
export function tableQuery(scope: OverviewScope, range: OverviewRange, dimension: Dimension): RumAggregateParams {
  return {
    ...view(scope, range),
    groupBy: [dimension.field],
    measures: [
      count('views'),
      ...METRICS.flatMap((metric) => [
        percentile(metric.key, metric.field),
        samples(`${metric.key}_samples`, metric.field),
        count(`${metric.key}_good`, [within(metric, '<=', metric.good)]),
      ]),
    ],
    orderBy: 'views',
    order: 'desc',
    limit: TABLE_ROWS,
  };
}

// ---------------------------------------------------------------------------------------------
// What the page reads out of the answers.

export interface Level {
  metric: Metric;
  /** In the unit the page reads it in; none when no view had it. */
  value: number | null;
  /** How many views had a value of it. */
  samples: number;
  good: number;
  poor: number;
  grade: Grade | null;
}

/** What the views in between are: those that had a value, less the good and the poor. */
export const needsImprovement = (level: Level): number => Math.max(0, level.samples - level.good - level.poor);

/** The levels an answer has, in the order they were asked for. */
export function readLevels(answer: RumAggregate | undefined, metrics: readonly Metric[]): Level[] {
  const row = answer?.rows[0];
  const found = answer && row ? named(answer, row) : {};

  return metrics.map((metric) => {
    const raw = found[metric.key] ?? null;
    const taken = found[`${metric.key}_samples`] ?? 0;
    const value = raw !== null && taken > 0 ? inUnit(metric, raw) : null;
    const good = Math.min(taken, found[`${metric.key}_good`] ?? 0);
    const poor = Math.min(taken - good, found[`${metric.key}_poor`] ?? 0);

    return { metric, value, samples: taken, good, poor, grade: value === null ? null : gradeOf(metric, value) };
  });
}

/** How many views there were. */
export function readViews(answer: RumAggregate | undefined): number {
  const row = answer?.rows[0];

  return (answer && row ? named(answer, row).views : null) ?? 0;
}

export interface Row {
  /** The value of the dimension; none where the views have none. */
  value: string | null;
  views: number;
  /** The 75th percentile of each level, in the unit the page reads it in. */
  levels: Record<MetricKey, number | null>;
  /** Of each level: how many views had it, and how many of those were good. */
  samples: Record<MetricKey, number>;
  good: Record<MetricKey, number>;
}

const byMetric = <T>(of: (metric: Metric) => T): Record<MetricKey, T> =>
  Object.fromEntries(METRICS.map((metric) => [metric.key, of(metric)])) as Record<MetricKey, T>;

export function readRows(answer: RumAggregate | undefined): Row[] {
  if (!answer) return [];

  return answer.rows.flatMap((row) => {
    const found = named(answer, row);

    if (found.views === null || found.views === undefined) return [];

    return [
      {
        value: row.group[0] || null,
        views: found.views,
        levels: byMetric((metric) => {
          const raw = found[metric.key] ?? null;

          return raw === null ? null : inUnit(metric, raw);
        }),
        samples: byMetric((metric) => found[`${metric.key}_samples`] ?? 0),
        good: byMetric((metric) => Math.min(found[`${metric.key}_samples`] ?? 0, found[`${metric.key}_good`] ?? 0)),
      },
    ];
  });
}

/** The share of the views that had a level, and had it good. */
export const goodShare = (row: Row, metric: MetricKey): number | null =>
  row.samples[metric] > 0 ? row.good[metric] / row.samples[metric] : null;

// ---------------------------------------------------------------------------------------------
// How the table is ordered.

export type SortKey = 'views' | MetricKey | 'good';

export const SORTS: readonly SortKey[] = ['views', ...METRICS.map((metric) => metric.key), 'good'];

export const DEFAULT_SORT: SortKey = 'views';

export const parseSort = (value: string | null): SortKey => SORTS.find((sort) => sort === value) ?? DEFAULT_SORT;

export type Direction = 'asc' | 'desc';

export const parseDirection = (value: string | null): Direction | undefined => (value === 'asc' || value === 'desc' ? value : undefined);

/** The order a column reads in when none is chosen: the most views, the slowest, and the lowest share of good ones first. */
export const defaultDirection = (sort: SortKey): Direction => (sort === 'good' ? 'asc' : 'desc');

/**
 * The rows in the order a reader chose. A ranking by a level or by the share of good views leaves the
 * rows with too few views to the end, whichever way it runs: a handful of views is not a slow page.
 */
export function sortRows(rows: readonly Row[], sort: SortKey, direction: Direction, metric: MetricKey): Row[] {
  const keyOf = (row: Row): number | null => (sort === 'views' ? row.views : sort === 'good' ? goodShare(row, metric) : row.levels[sort]);

  return [...rows].sort((a, b) => {
    if (sort !== 'views') {
      const fewA = a.views < MIN_VIEWS;
      const fewB = b.views < MIN_VIEWS;

      if (fewA !== fewB) return fewA ? 1 : -1;
    }

    const x = keyOf(a);
    const y = keyOf(b);

    if (x === y) return b.views - a.views;
    if (x === null) return 1;
    if (y === null) return -1;

    return direction === 'asc' ? x - y : y - x;
  });
}

// ---------------------------------------------------------------------------------------------
// What the chart draws.

/** What a level is drawn in, so that the lines where it turns poor read as it is told: 2.5 s, 200 ms. */
export interface Rescale {
  divisor: number;
  unit: string;
}

/** A time the SDK sends in nanoseconds is drawn in seconds from two and a half of them, else in milliseconds. */
export function rescaleOf(metric: Graded): Rescale | null {
  if (!metric.nanoseconds) return null;

  return metric.good >= 1_000 ? { divisor: 1_000_000_000, unit: 's' } : { divisor: 1_000_000, unit: 'ms' };
}

/** The lines where a level turns from good to poor, in the unit the numbers are drawn in. */
export function thresholdsOf(metric: Graded, labels: { good: string; poor: string }): TimeSeriesThreshold[] {
  const scale = metric.nanoseconds ? 1_000_000 / (rescaleOf(metric)?.divisor ?? 1) : 1;

  return [
    { value: metric.good * scale, label: labels.good, color: 'var(--green)', showLine: true },
    { value: metric.poor * scale, label: labels.poor, color: 'var(--red)', showLine: true },
  ];
}

// ---------------------------------------------------------------------------------------------
// What a group is made of: the part that is slow, the element, the views that were worst.

export interface Part {
  key: string;
  field: string;
}

export interface Attribution {
  /** The column of the element the level is set by: what was painted last, answered slowest, or moved. */
  selector: string;
  /** What the element is made of, where it is a file. */
  resource?: string;
  /** What the level is made of, in the order it happens; a shift is a score, not a time, and has none. */
  parts: readonly Part[];
}

export const ATTRIBUTIONS: Partial<Record<MetricKey, Attribution>> = {
  lcp: {
    selector: 'view.performance.lcp.target_selector',
    resource: 'view.performance.lcp.resource_url',
    parts: [
      { key: 'first_byte', field: 'view.first_byte' },
      { key: 'load_delay', field: 'view.performance.lcp.sub_parts.load_delay' },
      { key: 'load_time', field: 'view.performance.lcp.sub_parts.load_time' },
      { key: 'render_delay', field: 'view.performance.lcp.sub_parts.render_delay' },
    ],
  },
  inp: {
    selector: 'view.performance.inp.target_selector',
    parts: [
      { key: 'input_delay', field: 'view.performance.inp.sub_parts.input_delay' },
      { key: 'processing_duration', field: 'view.performance.inp.sub_parts.processing_duration' },
      { key: 'presentation_delay', field: 'view.performance.inp.sub_parts.presentation_delay' },
    ],
  },
  cls: { selector: 'view.performance.cls.target_selector', parts: [] },
};

/** The levels a drawer tells the makeup of: the vitals. */
export type DrawerMetric = VitalKey;

export const DRAWER_METRICS: readonly DrawerMetric[] = ['lcp', 'inp', 'cls'];

/** The tab a drawer opens on: the vital in front, or the first of them when the one in front is read beside them. */
export const drawerMetricOf = (value: string | null, fallback: MetricKey): DrawerMetric =>
  DRAWER_METRICS.find((key) => key === value) ?? DRAWER_METRICS.find((key) => key === fallback) ?? 'lcp';

/** How far around the 75th percentile the views lie that stand for it. */
export const COHORT = 0.1;

export const ELEMENT_ROWS = 6;
export const WORST_ROWS = 5;

/** The condition that makes the views of one value of a dimension. */
export const groupFilter = (dimension: Dimension, value: string): RumFilter => ({ field: dimension.field, op: '=', value });

/** The group's own 75th percentile, which its breakdown is taken around, and how many views it has and how many had the level. */
export function groupLevelQuery(scope: OverviewScope, range: OverviewRange, group: RumFilter, metric: Metric): RumAggregateParams {
  return {
    ...view(scope, range, [group]),
    measures: [count('views'), percentile('p75', metric.field), samples('samples', metric.field)],
  };
}

export interface GroupLevel {
  views: number;
  /** In the unit the page reads it in; none when no view of the group had the level. */
  value: number | null;
  /** As the SDK stores it: the breakdown is taken around it. */
  raw: number | null;
  samples: number;
}

export function readGroupLevel(answer: RumAggregate | undefined, metric: Metric): GroupLevel | null {
  const row = answer?.rows[0];

  if (!answer || !row) return null;

  const found = named(answer, row);
  const raw = found.p75 ?? null;
  const taken = found.samples ?? 0;

  return { views: found.views ?? 0, value: raw !== null && taken > 0 ? inUnit(metric, raw) : null, raw: taken > 0 ? raw : null, samples: taken };
}

/** The bound of the views that stand for the 75th percentile, in the unit the SDK stores. */
const bound = (metric: Pick<Metric, 'nanoseconds'>, raw: number, side: 'low' | 'high'): string => {
  // In whole percent, so that a level on a round number does not slip a unit over its line.
  const percent = Math.round(COHORT * 100);
  const scaled = (raw * (side === 'low' ? 100 - percent : 100 + percent)) / 100;

  return String(metric.nanoseconds ? (side === 'low' ? Math.floor(scaled) : Math.ceil(scaled)) : Number(scaled.toFixed(4)));
};

/** The conditions that take the rows whose value lies within a tenth of the 75th percentile, either way. */
export function cohortFilters(metric: Pick<Metric, 'field' | 'nanoseconds'>, raw: number): RumFilter[] {
  return [
    { field: metric.field, op: '>=', value: bound(metric, raw, 'low') },
    { field: metric.field, op: '<=', value: bound(metric, raw, 'high') },
  ];
}

/** What the views that stand for the 75th percentile are made of: the mean of each part. None for a level with no parts. */
export function partsQuery(
  scope: OverviewScope,
  range: OverviewRange,
  group: RumFilter,
  metric: Metric,
  raw: number,
): RumAggregateParams | null {
  const attribution = ATTRIBUTIONS[metric.key];

  if (!attribution || attribution.parts.length === 0) return null;

  return {
    ...view(scope, range, [group, ...cohortFilters(metric, raw)]),
    measures: [count('views'), ...attribution.parts.map((part): RumMeasure => ({ fn: 'avg', field: part.field, name: part.key }))],
  };
}

export interface Makeup {
  /** The views the parts were taken over. */
  views: number;
  /** In milliseconds, in the order they happen. */
  parts: Array<{ key: string; ms: number }>;
}

/**
 * What a level is made of, when the views say every part of it: a bar of some of the parts would not
 * add up to the level, and would read as if it did. A browser that reports the time to the first byte
 * but not the parts of the paint (Safari) leaves the level without a makeup.
 */
export function readMakeup(answer: RumAggregate | undefined, metric: Metric): Makeup | null {
  const attribution = ATTRIBUTIONS[metric.key];
  const row = answer?.rows[0];

  if (!answer || !row || !attribution) return null;

  const found = named(answer, row);
  const parts = attribution.parts.flatMap((part) => {
    const raw = found[part.key] ?? null;

    return raw === null || raw < 0 ? [] : [{ key: part.key, ms: inUnit({ nanoseconds: true }, raw) }];
  });

  return parts.length === 0 || parts.length < attribution.parts.length ? null : { views: found.views ?? 0, parts };
}

/** The part that took the longest, when there is a part that took any time. */
export function longest(makeup: Makeup): Makeup['parts'][number] | null {
  const best = makeup.parts.reduce<Makeup['parts'][number] | null>((top, part) => (top === null || part.ms > top.ms ? part : top), null);

  return best && best.ms > 0 ? best : null;
}

/** The elements that set the level in a group, the commonest first, with the file where it is one. */
export function elementsQuery(
  scope: OverviewScope,
  range: OverviewRange,
  group: RumFilter,
  metric: Metric,
): RumAggregateParams | null {
  const attribution = ATTRIBUTIONS[metric.key];

  if (!attribution) return null;

  return {
    ...view(scope, range, [group, exists(attribution.selector)]),
    groupBy: [attribution.selector, ...(attribution.resource ? [attribution.resource] : [])],
    measures: [count('views'), percentile('p75', metric.field)],
    orderBy: 'views',
    order: 'desc',
    limit: ELEMENT_ROWS,
  };
}

export interface Element {
  selector: string;
  /** The file of an image or a video; none for text. */
  resource: string | null;
  views: number;
  /** The 75th percentile of the views that had this element, in the unit the page reads it in. */
  value: number | null;
}

export function readElements(answer: RumAggregate | undefined, metric: Metric): Element[] {
  if (!answer) return [];

  return answer.rows.flatMap((row) => {
    const selector = row.group[0];
    const found = named(answer, row);

    if (!selector || found.views === null || found.views === undefined) return [];

    const raw = found.p75 ?? null;

    return [{ selector, resource: row.group[1] || null, views: found.views, value: raw === null ? null : inUnit(metric, raw) }];
  });
}

/** The views of a group that had the highest value of a level: which they were, and how high. */
export function worstQuery(scope: OverviewScope, range: OverviewRange, group: RumFilter, metric: Metric): RumAggregateParams {
  return {
    ...view(scope, range, [group, exists(metric.field)]),
    groupBy: ['view.id'],
    measures: [{ fn: 'max', field: metric.field, name: 'worst' }],
    orderBy: 'worst',
    order: 'desc',
    limit: WORST_ROWS,
  };
}

export interface Worst {
  id: string;
  /** In the unit the page reads it in. */
  value: number;
}

export function readWorst(answer: RumAggregate | undefined, metric: Metric): Worst[] {
  if (!answer) return [];

  return answer.rows.flatMap((row) => {
    const id = row.group[0];
    const raw = named(answer, row).worst;

    return id && raw !== null && raw !== undefined ? [{ id, value: inUnit(metric, raw) }] : [];
  });
}

/** The columns of the views that were worst, to say when and for whom. */
export const WORST_COLUMNS = [
  'view.id',
  'session.id',
  'view.url_path',
  'usr.id',
  'usr.name',
  'usr.email',
  'usr.anonymous_id',
  'version',
  'browser.name',
  'browser.version',
  'os.name',
] as const;

/** The views with these ids, in the range, as rows. */
export function worstRowsQuery(scope: OverviewScope, range: OverviewRange, ids: readonly string[]): RumSearchParams {
  return {
    ...view(scope, range, [{ field: 'view.id', op: 'in', value: JSON.stringify(ids) }]),
    columns: [...WORST_COLUMNS],
    limit: ids.length,
  };
}

export interface WorstView extends Worst {
  /** When the view began, in microseconds. */
  at: number | null;
  session: string | null;
  page: string | null;
  /** A name, an address, an id, or nobody known. */
  user: string | null;
  version: string | null;
  browser: string | null;
  system: string | null;
}

const text = (row: RumRow | undefined, field: string): string | null => {
  const value = row?.[field];

  return typeof value === 'string' && value.trim() !== '' ? value : null;
};

/** The worst views, each with what the row of the view says about it. */
export function worstViews(worst: readonly Worst[], rows: readonly RumRow[]): WorstView[] {
  return worst.map((entry) => {
    const row = rows.find((candidate) => candidate['view.id'] === entry.id);
    const browser = [text(row, 'browser.name'), text(row, 'browser.version')].filter(Boolean).join(' ');

    return {
      ...entry,
      at: numeric(row?.timestamp),
      session: text(row, 'session.id'),
      page: text(row, 'view.url_path'),
      user: text(row, 'usr.name') ?? text(row, 'usr.email') ?? text(row, 'usr.id') ?? text(row, 'usr.anonymous_id'),
      version: text(row, 'version'),
      browser: browser || null,
      system: text(row, 'os.name'),
    };
  });
}
