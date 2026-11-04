/**
 * The aggregate read of the mock RUM backend: numbers over the rows a selection matches, per group
 * and per bar of time, by the rules of the server's `GET /rum/aggregate` (see
 * `infra/rum/query/aggregate.rs`). The mock rows are one per view and per session already, so
 * there is no latest revision to take. Percentiles are by nearest rank.
 *
 * A row may say the share it was kept with (`retention.rate`, in percent): an estimate counts it for
 * as many as that says, as the server does, and `stored` counts the rows as they are.
 */
import type { RumRow, RumSchema } from '../../src/api/rum';

export interface Filter {
  field: string;
  op: string;
  value?: string;
}

interface Measure {
  fn: string;
  field?: string;
  fallback?: string;
  where?: Filter[];
  name?: string;
}

export interface AggregateAsked {
  measures: Measure[];
  groupBy: string[];
  bucketTime?: number | undefined;
  originTime?: number | undefined;
  orderBy?: string | undefined;
  order?: string | undefined;
  limit?: number | undefined;
  /** Count what the rows stand for (the default), or the rows. */
  estimate?: boolean | undefined;
}

const present = (value: unknown) => value !== undefined && value !== null && value !== '';
const text = (value: unknown): string | null => (present(value) ? String(value) : null);

const PERCENTILES: Record<string, number> = { p50: 0.5, p75: 0.75, p90: 0.9, p95: 0.95, p99: 0.99 };
const FUNCTIONS = ['count', 'stored', 'count_distinct', 'sum', 'avg', 'min', 'max', ...Object.keys(PERCENTILES)];

/** How many rows a row stands for: a row kept with a share of 25 stands for four, one that says none for one. */
const weightOf = (row: RumRow): number => {
  const rate = Number(row['retention.rate']);

  return Number.isFinite(rate) && rate > 0 ? 100 / rate : 1;
};

export class Invalid extends Error {}

/** A key of a JSON document, which the server reads like a column. */
const isMember = (field: string) => ['feature_flags.', 'context.', 'view.custom_timings.', 'usr.extra.'].some((prefix) => field.startsWith(prefix));

const nameOf = (measure: Measure) => measure.name ?? (measure.field ? `${measure.fn}(${measure.field})` : measure.fn);

type Holds = (row: RumRow, filter: Filter) => boolean;

function valueOf(rows: readonly RumRow[], measure: Measure, holds: Holds, estimate: boolean): number | null {
  const taken = measure.where?.length ? rows.filter((row) => measure.where!.every((filter) => holds(row, filter))) : rows;
  const field = measure.field;
  const weight = (row: RumRow) => (estimate ? weightOf(row) : 1);
  // The values of the rows that have one, each with the number of rows it stands for.
  const pairs = () =>
    taken
      .filter((row) => (field ? present(row[field]) : false))
      .map((row) => ({ value: Number(row[field as string]), weight: weight(row) }));
  const total = (list: Array<{ weight: number }>) => list.reduce((sum, item) => sum + item.weight, 0);

  switch (measure.fn) {
    case 'stored':
      return field ? taken.filter((row) => present(row[field])).length : taken.length;
    case 'count':
      return Math.round(total((field ? taken.filter((row) => present(row[field])) : taken).map((row) => ({ weight: weight(row) }))));
    case 'count_distinct': {
      const keys = taken.map((row) => (field ? text(row[field]) : null) ?? (measure.fallback ? text(row[measure.fallback]) : null));
      const distinct = new Set(keys.filter((key) => key !== null)).size;

      // The sessions that were seen stand for as many as the mean of their rows says; people are as they were seen.
      return estimate && field === 'session.id' && taken.length > 0 ? Math.round((distinct * total(taken.map((row) => ({ weight: weight(row) })))) / taken.length) : distinct;
    }
    case 'sum':
      return pairs().reduce((sum, item) => sum + item.value * item.weight, 0);
    case 'avg': {
      const all = pairs();

      return all.length === 0 ? null : all.reduce((sum, item) => sum + item.value * item.weight, 0) / total(all);
    }
    case 'min':
      return pairs().length === 0 ? null : Math.min(...pairs().map((item) => item.value));
    case 'max':
      return pairs().length === 0 ? null : Math.max(...pairs().map((item) => item.value));
    default: {
      const share = PERCENTILES[measure.fn] as number;
      const sorted = pairs().sort((a, b) => a.value - b.value);

      if (sorted.length === 0) return null;

      // The first value that this share of the rows they stand for is at or under, by nearest rank.
      const wanted = Math.max(share * total(sorted), Number.EPSILON);
      let seen = 0;

      for (const item of sorted) {
        seen += item.weight;

        if (seen >= wanted - 1e-9) return item.value;
      }

      return (sorted.at(-1) as { value: number }).value;
    }
  }
}

const compare = (a: string | null, b: string | null) => (a === b ? 0 : a === null ? 1 : b === null ? -1 : a.localeCompare(b));

/** The answer of the server for the rows a selection matched. */
export function aggregateAnswer(rows: readonly RumRow[], schema: RumSchema, asked: AggregateAsked, holds: Holds, range: { from: number }) {
  if (asked.measures.length === 0) throw new Invalid('at least one measure is required');
  if (asked.groupBy.length > 2) throw new Invalid('group_by takes at most 2 fields');

  for (const measure of asked.measures) {
    if (!FUNCTIONS.includes(measure.fn)) throw new Invalid(`unsupported measure \`${measure.fn}\``);
    for (const field of [measure.field, measure.fallback, ...asked.groupBy]) {
      if (field && !isMember(field) && !schema.fields.some((column) => column.name === field)) throw new Invalid(`\`${field}\` is not a field of ${schema.stream}`);
    }
  }

  const unit = (measure: Measure) =>
    measure.fn.startsWith('count') || measure.fn === 'stored' ? 'count' : (schema.fields.find((column) => column.name === measure.field)?.unit ?? 'none');

  const measures = asked.measures.map((measure) => ({
    name: nameOf(measure),
    fn: measure.fn,
    ...(measure.field ? { field: measure.field } : {}),
    unit: unit(measure),
  }));

  const groupOf = (row: RumRow) => asked.groupBy.map((field) => text(row[field]));
  const keyOf = (group: Array<string | null>) => JSON.stringify(group);

  const grouped = new Map<string, { group: Array<string | null>; rows: RumRow[] }>();

  // Nothing grouped is one row over every row, however few there are, as the server's is.
  if (asked.groupBy.length === 0 && !asked.bucketTime) grouped.set(keyOf([]), { group: [], rows: [] });

  for (const row of rows) {
    const group = groupOf(row);
    const entry = grouped.get(keyOf(group)) ?? { group, rows: [] };

    entry.rows.push(row);
    grouped.set(keyOf(group), entry);
  }

  const estimate = asked.estimate !== false;
  const numbers = (entry: { rows: RumRow[] }) => asked.measures.map((measure) => valueOf(entry.rows, measure, holds, estimate));

  const by = asked.orderBy ?? '';
  const measureIndex = measures.findIndex((measure) => measure.name === by);
  const groupIndex = asked.groupBy.indexOf(by);
  const descending = asked.order ? asked.order === 'desc' : groupIndex < 0;

  const ranked = [...grouped.values()]
    .map((entry) => ({ ...entry, values: numbers(entry) }))
    .sort((a, b) => {
      const key = groupIndex >= 0 ? 0 : (measureIndex >= 0 ? measureIndex : 0);
      const first =
        groupIndex >= 0
          ? compare(a.group[groupIndex] ?? null, b.group[groupIndex] ?? null)
          : ((a.values[key] ?? -Infinity) as number) - ((b.values[key] ?? -Infinity) as number);

      if (first !== 0) return descending ? -first : first;

      return a.group.map((value, index) => compare(value, b.group[index] ?? null)).find((order) => order !== 0) ?? 0;
    });

  if (!asked.bucketTime) {
    return {
      groupBy: asked.groupBy,
      measures,
      rows: ranked.slice(0, asked.groupBy.length === 0 ? 1 : (asked.limit ?? 10)).map((entry) => ({ group: entry.group, values: entry.values })),
    };
  }

  // Over time: the groups that rank first are followed through the bars.
  const bucket = asked.bucketTime;
  const origin = asked.originTime ?? range.from;
  const followed = asked.groupBy.length === 0 ? [{ group: [] as Array<string | null>, rows: [...rows] }] : ranked.slice(0, Math.min(asked.limit ?? 10, 20));
  const bars = followed.flatMap((entry) => {
    const byBar = new Map<number, RumRow[]>();

    for (const row of entry.rows) {
      const index = Math.floor((row.timestamp - origin) / bucket);

      byBar.set(index, [...(byBar.get(index) ?? []), row]);
    }

    return [...byBar].map(([index, inBar]) => ({
      group: entry.group,
      startTime: origin + index * bucket,
      values: asked.measures.map((measure) => valueOf(inBar, measure, holds, estimate)),
    }));
  });

  bars.sort((a, b) => a.startTime - b.startTime);

  return { groupBy: asked.groupBy, measures, bucketTime: bucket, originTime: origin, rows: bars };
}
