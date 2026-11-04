import type { RumAggregateParams, RumField, RumFilter, RumMeasureFn, RumQuery } from '@/api/rum';
import { alignDown, alignUp, binWidthFor, type HistogramRange } from '@/shell/query/histogram/model';

import type { RumTab } from '../model';
import type { RumChart, RumVizAddress } from './address';

/** The field the map is drawn by: every type of row carries where its client was. */
export const COUNTRY_FIELD = 'geo.country_iso_code';

export const MEASURE_FNS: readonly RumMeasureFn[] = [
  'count',
  'count_distinct',
  'sum',
  'avg',
  'min',
  'max',
  'p50',
  'p75',
  'p90',
  'p95',
  'p99',
];

/** One number asked of the rows: a function, and the field it takes unless it counts rows. */
export interface RumMeasureChoice {
  fn: RumMeasureFn;
  field?: string;
}

export const COUNT: RumMeasureChoice = { fn: 'count' };

/** What the server calls a measure, and what a table is ordered by when it is told one. */
export function measureName(measure: RumMeasureChoice): string {
  return measure.field ? `${measure.fn}(${measure.field})` : measure.fn;
}

export function measureToken(measure: RumMeasureChoice): string {
  return measure.field ? `${measure.fn}:${measure.field}` : measure.fn;
}

/** Whether a measure adds up: the bars of two groups can be stacked, and a share of a total means something. */
export function isAdditive(fn: RumMeasureFn): boolean {
  return fn === 'count' || fn === 'count_distinct' || fn === 'sum';
}

const isNumber = (field: RumField) => field.dataType === 'int64' || field.dataType === 'float64';

/** The fields a function can take: none for `count`, any with values for `count_distinct`, a number for the rest. */
export function measureFields(fn: RumMeasureFn, fields: readonly RumField[]): RumField[] {
  if (fn === 'count') return [];
  if (fn === 'count_distinct') return fields.filter((field) => field.facetable);

  return fields.filter(isNumber);
}

/** What a field is measured in, when what it holds is a measurement and not something that repeats. */
const MEASUREMENT_UNITS: ReadonlySet<RumField['unit']> = new Set(['ns', 'ms', 'bytes', 'percent', 'score', 'px', 'per_second']);

/** The fields the rows can be grouped by: a value that repeats (a name, a status), not a measurement or a time. */
export function groupFields(fields: readonly RumField[]): RumField[] {
  return fields.filter(
    (field) =>
      field.facetable &&
      (field.dataType === 'utf8' || field.dataType === 'bool' || field.dataType === 'int64') &&
      !MEASUREMENT_UNITS.has(field.unit),
  );
}

export function measureOfToken(token: string, fields: readonly RumField[]): RumMeasureChoice | null {
  const [head = '', ...rest] = token.split(':');
  const fn = MEASURE_FNS.find((candidate) => candidate === head);
  const field = rest.join(':');

  if (!fn) return null;
  if (fn === 'count') return field ? null : COUNT;
  if (!field) return null;

  return measureFields(fn, fields).some((candidate) => candidate.name === field) ? { fn, field } : null;
}

/** What a type is most often ranked by: where its rows came from or what they are of. */
const DEFAULT_GROUP: Record<RumTab, string> = {
  sessions: 'browser.name',
  views: 'view.url_path_group',
  actions: 'action.target.name',
  errors: 'error.type',
  resources: 'resource.url_host',
  long_tasks: 'view.url_path_group',
  logs: 'level',
};

const FALLBACK_GROUPS = ['browser.name', 'os.name', 'device.type', 'env', 'version'];

/** The field a ranking opens with: the type's own, else the first the schema has of the common ones. */
export function defaultGroup(tab: RumTab, fields: readonly RumField[]): string | null {
  const names = new Set(groupFields(fields).map((field) => field.name));

  return [DEFAULT_GROUP[tab], ...FALLBACK_GROUPS].find((name) => names.has(name)) ?? null;
}

/** What a type is most often told by as a number: how long it took, or how much it was. */
const DEFAULT_NUMBER: Record<RumTab, string> = {
  sessions: 'session.duration',
  views: 'view.loading_time',
  actions: 'action.loading_time',
  errors: 'display.viewport.width',
  resources: 'resource.duration',
  long_tasks: 'long_task.duration',
  logs: 'display.viewport.width',
};

/** The fields a distribution can be of: numbers. */
export function numberFields(fields: readonly RumField[]): RumField[] {
  return fields.filter(isNumber);
}

/** The number a distribution is of: the one asked for as far as the type has it, else the type's own. */
function numberOf(tab: RumTab, asked: string | null, fields: readonly RumField[]): string | null {
  const numbers = numberFields(fields);
  const known = (name: string | null) => numbers.some((field) => field.name === name);

  if (known(asked)) return asked;
  if (known(DEFAULT_NUMBER[tab])) return DEFAULT_NUMBER[tab];

  return numbers[0]?.name ?? null;
}

/** How many measures a way of drawing takes: one number is drawn, a table has a column for each. */
export const MAX_MEASURES: Record<RumChart, number> = { timeseries: 1, toplist: 1, table: 6, map: 1, distribution: 0 };

/** How many fields the rows are grouped by. */
export const MAX_GROUPS: Record<RumChart, number> = { timeseries: 2, toplist: 1, table: 2, map: 1, distribution: 0 };

const TOPS: Record<RumChart, readonly number[]> = {
  timeseries: [3, 5, 10, 20],
  toplist: [5, 10, 20, 50, 100],
  table: [5, 10, 20, 50, 100],
  map: [],
  distribution: [],
};

const DEFAULT_TOP: Record<RumChart, number> = { timeseries: 5, toplist: 10, table: 10, map: 250, distribution: 0 };

/** The numbers of groups a reader can ask for in a way of drawing. */
export const topChoices = (viz: RumChart): readonly number[] => TOPS[viz];

function topOf(viz: RumChart, asked: number | null): number {
  const options = TOPS[viz];

  if (asked === null || options.length === 0) return DEFAULT_TOP[viz];

  return [...options].reverse().find((option) => option <= asked) ?? options[0]!;
}

const SECOND = 1_000_000;
const MINUTE = 60 * SECOND;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

/** The lengths of time a reader can ask a bar to be, by the word the address says them in. */
export const BUCKETS: ReadonlyArray<{ token: string; micros: number }> = [
  { token: '10s', micros: 10 * SECOND },
  { token: '30s', micros: 30 * SECOND },
  { token: '1m', micros: MINUTE },
  { token: '5m', micros: 5 * MINUTE },
  { token: '15m', micros: 15 * MINUTE },
  { token: '30m', micros: 30 * MINUTE },
  { token: '1h', micros: HOUR },
  { token: '3h', micros: 3 * HOUR },
  { token: '6h', micros: 6 * HOUR },
  { token: '12h', micros: 12 * HOUR },
  { token: '1d', micros: DAY },
  { token: '7d', micros: 7 * DAY },
];

export const bucketOfToken = (token: string | null): number | null =>
  BUCKETS.find((bucket) => bucket.token === token)?.micros ?? null;

export const tokenOfBucket = (micros: number | null): string | null =>
  BUCKETS.find((bucket) => bucket.micros === micros)?.token ?? null;

/** About how many bars a range is cut into when the reader leaves the length of a bar to it. */
const AUTO_BARS = 30;
/** The most bars a chart is given: the server takes 512, and a canvas has no use for so many. */
const MAX_BARS = 300;

export interface RumVizBars {
  /** What one bar stands for, in microseconds. */
  binMicros: number;
  /** The range widened to whole bars, each beginning on a multiple of its length on the reader's clock. */
  frame: HistogramRange;
}

/**
 * The bars a range is drawn in. A length the reader chose that would cut the range into more
 * bars than a chart carries is replaced by the shortest that does not.
 */
export function barsFor(range: HistogramRange, bucket: number | null, utcOffsetMicros: number): RumVizBars {
  const span = Math.max(1, range.end - range.start);
  const wanted = bucket ?? binWidthFor(span, AUTO_BARS);
  const binMicros = Math.ceil(span / wanted) > MAX_BARS ? binWidthFor(span, MAX_BARS) : wanted;
  const start = alignDown(range.start, binMicros, utcOffsetMicros);
  const end = Math.max(alignUp(range.end, binMicros, utcOffsetMicros), start + binMicros);

  return { binMicros, frame: { start, end } };
}

/** What a bar stands for when the reader leaves its length to the range. */
export function autoBucketOf(range: HistogramRange): number {
  return barsFor(range, null, 0).binMicros;
}

/** The lengths a reader can choose for a range: each cuts it into a few bars or many, not into one or a thousand. */
export function bucketChoices(range: HistogramRange): number[] {
  const span = Math.max(1, range.end - range.start);

  return BUCKETS.filter((bucket) => {
    const bars = Math.ceil(span / bucket.micros);
    return bars >= 2 && bars <= MAX_BARS;
  }).map((bucket) => bucket.micros);
}

/** The numbers asked for, read against the fields of a type and made whole. */
export interface RumVizSpec {
  viz: RumChart;
  measures: RumMeasureChoice[];
  groups: string[];
  /** The length of a bar the reader chose; none leaves it to the range. */
  bucket: number | null;
  top: number;
  /** What a table is ordered by when the reader chose a column. */
  sort: { by: string; descending: boolean } | null;
  /** The number a distribution is of. */
  field: string | null;
}

const unique = <T>(items: readonly T[], key: (item: T) => string): T[] => {
  const seen = new Set<string>();

  return items.filter((item) => {
    const id = key(item);
    if (seen.has(id)) return false;
    seen.add(id);
    return true;
  });
};

/**
 * What an address asks to be drawn, against the fields of the type. What it asks that the type
 * cannot answer is left out, and what it leaves out is the type's own: a count, the groups a
 * ranking opens with, five groups of a chart and ten of a list.
 */
export function resolveVizSpec(
  viz: RumChart,
  address: RumVizAddress,
  tab: RumTab,
  fields: readonly RumField[],
): RumVizSpec {
  const asked = address.measures.flatMap((token) => {
    const measure = measureOfToken(token, fields);
    return measure ? [measure] : [];
  });
  const measures = unique(asked, measureName).slice(0, MAX_MEASURES[viz]);

  if (measures.length === 0 && viz !== 'distribution') measures.push(COUNT);

  const known = new Set(groupFields(fields).map((field) => field.name));
  const stored = unique(
    address.groups.filter((name) => known.has(name)),
    (name) => name,
  ).slice(0, MAX_GROUPS[viz]);
  const fallback = defaultGroup(tab, fields);

  // A chart can stand for the whole; a ranking, a table and a map are of some groups.
  const groups =
    viz === 'distribution'
      ? []
      : viz === 'map'
        ? [COUNTRY_FIELD]
        : viz === 'timeseries' || stored.length > 0
          ? stored
          : fallback
            ? [fallback]
            : [];

  const named = address.sort;
  const sortable = viz === 'table' && named !== null && (measures.some((m) => measureName(m) === named) || groups.includes(named));

  return {
    viz,
    measures,
    groups,
    bucket: bucketOfToken(address.bucket),
    top: topOf(viz, address.top),
    sort: sortable ? { by: named, descending: address.descending ?? !groups.includes(named) } : null,
    field: viz === 'distribution' ? numberOf(tab, address.field, fields) : null,
  };
}

/** What the numbers are asked over: the rows the query matches, in the range. */
export interface RumVizContext {
  tab: RumTab;
  /** The range the list is of, in microseconds. */
  range: { from: number; to: number };
  filters: readonly RumFilter[];
  freeText: readonly string[];
  /** How far the reader's clock is ahead of UTC: bars begin on its hours and days. */
  utcOffsetMicros: number;
}

export interface RumVizRequest {
  params: RumAggregateParams;
  /** The bars of a chart over time. */
  bars: RumVizBars | null;
}

/** The read that answers a distribution; none while the type has no number to tell. */
export function distributionRequest(spec: RumVizSpec, context: RumVizContext): (RumQuery & { field: string }) | null {
  if (spec.viz !== 'distribution' || !spec.field) return null;

  return {
    type: context.tab,
    from: context.range.from,
    to: context.range.to,
    field: spec.field,
    ...(context.filters.length > 0 ? { filters: [...context.filters] } : {}),
    ...(context.freeText.length > 0 ? { freeText: [...context.freeText] } : {}),
  };
}

/** The read of the aggregate that answers a spec. */
export function vizRequest(spec: RumVizSpec, context: RumVizContext): RumVizRequest {
  const range = { start: context.range.from, end: context.range.to };
  // A length that does not cut this range into a chart's worth of bars is not one of its choices.
  const bucket = spec.bucket !== null && bucketChoices(range).includes(spec.bucket) ? spec.bucket : null;
  const bars = spec.viz === 'timeseries' ? barsFor(range, bucket, context.utcOffsetMicros) : null;

  return {
    bars,
    params: {
      type: context.tab,
      from: bars ? bars.frame.start : context.range.from,
      to: bars ? bars.frame.end : context.range.to,
      ...(context.filters.length > 0 ? { filters: [...context.filters] } : {}),
      ...(context.freeText.length > 0 ? { freeText: [...context.freeText] } : {}),
      measures: spec.measures.map((measure) => (measure.field ? { fn: measure.fn, field: measure.field } : { fn: measure.fn })),
      ...(spec.groups.length > 0 ? { groupBy: spec.groups } : {}),
      ...(bars ? { bucketTime: bars.binMicros, originTime: bars.frame.start } : {}),
      ...(spec.sort ? { orderBy: spec.sort.by, order: spec.sort.descending ? 'desc' : 'asc' } : {}),
      limit: spec.viz === 'timeseries' ? Math.min(spec.top, TOPS.timeseries.at(-1)!) : spec.top,
      // The explorer lists the rows that were kept, and what it draws counts those, not what they stand for.
      estimate: false,
    },
  };
}
