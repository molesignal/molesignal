import type { TraceRedFigures, TraceRedLatencyBar, TraceRedPoint } from '@/api/traceRed';
import type { TraceFilter } from '@/api/web';
import { joinFieldConjuncts } from '@/shell/query/fieldExpression';
import { appendAttributeClause, attributeClause } from '@/viz/trace/span-detail/model';

import { splitTraceStatement, traceClauseOf, type TraceFieldDef } from '../fieldQueryModel';

/**
 * The fields a breakdown offers first, in the order a drill goes down them:
 * the service, what it was asked to do, how it answered, where it runs.
 */
export const RED_DIMENSION_ORDER: readonly string[] = [
  'service.name',
  'name',
  'http.route',
  'http.request.method',
  'http.response.status_code',
  'http.status_code',
  'rpc.method',
  'status_code',
  'service.version',
  'deployment.environment.name',
  'deployment.environment',
  'k8s.pod.name',
  'host.name',
  'service.instance.id',
  'kind',
];

/** Fields whose values are all but different for every Span: they group nothing. */
const UNGROUPED_FIELDS = new Set([
  'trace_id',
  'span_id',
  'parent_span_id',
  'trace_state',
  'status_message',
  'start_time_unix_nano',
  'end_time_unix_nano',
  'duration_ns',
]);

const GROUPED_TYPES = new Set<TraceFieldDef['dataType']>(['utf8', 'int64', 'float64', 'bool']);

/** The fields requests can be broken down by: the preferred ones in drill order, then the rest by name. */
export function groupableFields(fields: readonly TraceFieldDef[]): TraceFieldDef[] {
  const usable = fields.filter(
    (field) =>
      field.physical &&
      field.scope === 'span' &&
      GROUPED_TYPES.has(field.dataType) &&
      !UNGROUPED_FIELDS.has(field.name) &&
      !field.name.startsWith('molesignal.trace.'),
  );
  const rank = (name: string) => {
    const index = RED_DIMENSION_ORDER.indexOf(name);
    return index < 0 ? RED_DIMENSION_ORDER.length : index;
  };

  return usable.sort((left, right) => rank(left.name) - rank(right.name) || left.name.localeCompare(right.name));
}

/** The fields the query already holds to one value: grouping by them would show one row. */
export function pinnedFields(filters: readonly TraceFilter[]): Set<string> {
  return new Set(filters.filter((filter) => filter.op === '=').map((filter) => filter.field));
}

/**
 * The field to break the requests down by: the one asked for while the stream
 * has it, otherwise the first of the drill order the query does not hold to
 * one value yet.
 */
export function dimensionFor(
  requested: string | null,
  fields: readonly TraceFieldDef[],
  filters: readonly TraceFilter[],
): string | null {
  const groupable = groupableFields(fields);
  if (requested && groupable.some((field) => field.name === requested)) return requested;

  const pinned = pinnedFields(filters);

  return (groupable.find((field) => !pinned.has(field.name)) ?? groupable[0])?.name ?? null;
}

/** The field a drill goes on to after holding `field` to a value. */
export function nextDimension(
  field: string,
  fields: readonly TraceFieldDef[],
  filters: readonly TraceFilter[],
): string {
  const pinned = pinnedFields(filters);
  pinned.add(field);
  const groupable = groupableFields(fields);
  const after = groupable.findIndex((candidate) => candidate.name === field);
  const ordered = [...groupable.slice(after + 1), ...groupable.slice(0, Math.max(after, 0))];
  const next = ordered.find(
    (candidate) => RED_DIMENSION_ORDER.includes(candidate.name) && !pinned.has(candidate.name),
  );

  return next?.name ?? field;
}

/** One step of the drill path: a part of the field query as it is written. */
export interface DrillStep {
  text: string;
  /** The comparison it makes; absent for free text. */
  clause: TraceFilter | null;
}

/** The field query read as the steps that narrowed it, in the order they were written. */
export function drillSteps(statement: string): DrillStep[] {
  return splitTraceStatement(statement.trim())
    .map((part) => part.trim())
    .filter(Boolean)
    .map((text) => ({ text, clause: traceClauseOf(text) }));
}

/** The query with only its first `count` steps: back up the path to there. */
export function keepSteps(statement: string, count: number): string {
  return joinFieldConjuncts(
    drillSteps(statement)
      .slice(0, Math.max(0, count))
      .map((step) => step.text),
  );
}

/** The query without step `index`, the steps after it kept. */
export function withoutStep(statement: string, index: number): string {
  return joinFieldConjuncts(
    drillSteps(statement)
      .filter((_, position) => position !== index)
      .map((step) => step.text),
  );
}

/** The query narrowed to the requests whose `field` has `value`, written as its type reads. */
export function drillInto(statement: string, field: TraceFieldDef | undefined, name: string, value: string): string {
  const typed =
    field?.dataType === 'bool' && (value === 'true' || value === 'false')
      ? value === 'true'
      : (field?.dataType === 'int64' || field?.dataType === 'float64') && Number.isFinite(Number(value)) && value.trim()
        ? Number(value)
        : value;
  const clause = attributeClause(name, typed, '=');
  const steps = drillSteps(statement);
  if (steps.some((step) => step.text === clause)) return statement.trim();

  return appendAttributeClause(statement, clause);
}

/** The query narrowed to the requests that took from `fromNs` up to `toNs` (no end: longer). */
export function narrowDuration(statement: string, fromNs: number, toNs: number | null): string {
  const kept = joinFieldConjuncts(
    drillSteps(statement)
      .filter((step) => step.clause?.field !== 'duration_ns')
      .map((step) => step.text),
  );
  const lower = fromNs > 0 ? appendAttributeClause(kept, `duration_ns >= ${Math.round(fromNs)}`) : kept;

  return toNs === null ? lower : appendAttributeClause(lower, `duration_ns < ${Math.round(toNs)}`);
}

/** The query narrowed to the requests that failed. */
export function narrowToFailures(statement: string): string {
  const clause = attributeClause('status_code', 'ERROR', '=');
  const steps = drillSteps(statement);
  if (steps.some((step) => step.clause?.field === 'status_code' && step.clause.op === '=' && step.clause.value.toUpperCase() === 'ERROR')) {
    return statement.trim();
  }

  return appendAttributeClause(statement, clause);
}

/** A rate as a reader counts it: per second, or per minute or hour when that is under one. */
export interface RateUnit {
  seconds: number;
  key: 'per_second' | 'per_minute' | 'per_hour';
}

const RATE_UNITS: readonly RateUnit[] = [
  { seconds: 1, key: 'per_second' },
  { seconds: 60, key: 'per_minute' },
  { seconds: 3_600, key: 'per_hour' },
];

/** The unit that says the highest of `perSecond` rates as at least one. */
export function rateUnitFor(perSecond: readonly number[]): RateUnit {
  const highest = Math.max(0, ...perSecond.filter(Number.isFinite));

  return RATE_UNITS.find((unit) => highest * unit.seconds >= 1) ?? RATE_UNITS[RATE_UNITS.length - 1]!;
}

/** The share of `figures` that failed; `null` where that cannot be told. */
export function errorRate(figures: Pick<TraceRedFigures, 'requests' | 'errors'>): number | null {
  if (figures.errors === null || figures.requests === 0) return null;

  return figures.errors / figures.requests;
}

/** A value for each bar of a range, from the bars that hold something; `empty` for the rest. */
export function everyBar<T extends { startTime: number }, V>(
  points: readonly T[],
  bars: { originTime: number; bucketTime: number; count: number },
  read: (point: T) => V,
  empty: V,
): V[] {
  const values = Array.from({ length: bars.count }, () => empty);

  for (const point of points) {
    const index = (point.startTime - bars.originTime) / bars.bucketTime;
    if (Number.isInteger(index) && index >= 0 && index < bars.count) values[index] = read(point);
  }

  return values;
}

/** The value a group's trend follows for the figure in front: requests, the share failed, the 95th percentile. */
export function pointValue(point: TraceRedPoint, signal: RedSignal): number | null {
  if (signal === 'rate') return point.requests;
  if (signal === 'errors') return errorRate(point);

  return point.p95Ns;
}

/**
 * The groups in the order of the figure in front: the most requests, the
 * largest share failed, or the slowest twentieth first. A figure that cannot
 * be told goes last; the most requests break a tie.
 */
export function sortGroups<T extends TraceRedFigures>(groups: readonly T[], signal: RedSignal): T[] {
  const key = (group: T): number | null => {
    if (signal === 'rate') return group.requests;
    if (signal === 'errors') return errorRate(group);

    return group.p95Ns;
  };

  return [...groups].sort((left, right) => {
    const a = key(left);
    const b = key(right);
    if (a === null || b === null) {
      if (a !== b) return a === null ? 1 : -1;
    } else if (a !== b) {
      return b - a;
    }

    return right.requests - left.requests;
  });
}

/** Which of the three figures leads: the breakdown is sorted and followed by it. */
export type RedSignal = 'rate' | 'errors' | 'duration';

export const RED_SIGNALS: readonly RedSignal[] = ['rate', 'errors', 'duration'];

/** The preferred numbers of the R10 series in hundreds: ten round steps to a power of ten. */
const PREFERRED = [100, 125, 160, 200, 250, 315, 400, 500, 630, 800];

/**
 * Where the bars of the duration distribution begin, in nanoseconds, as the
 * backend counts them: the R10 series in every power of ten from 100 ns up to
 * 1,000 s. The first bar also holds whatever is shorter, the last whatever is
 * longer.
 */
export const LATENCY_EDGES_NS: readonly number[] = [
  ...Array.from({ length: 10 }, (_, decade) => PREFERRED.map((edge) => edge * 10 ** decade)).flat(),
  PREFERRED[0]! * 10 ** 10,
];

/** The edges where a power of ten begins: the ones the duration axis is labelled at. */
export const LATENCY_DECADES_NS: ReadonlySet<number> = new Set(
  LATENCY_EDGES_NS.filter((_, index) => index % PREFERRED.length === 0),
);

/** Fewer bars than this say little of a shape: the axis is widened around them to this many. */
const MIN_LATENCY_SLOTS = 12;

/** One bar of the duration distribution, the failures apart from the rest. */
export interface LatencySlot {
  fromNs: number;
  /** `null` for the last bar, which holds every longer duration. */
  toNs: number | null;
  ok: number;
  errors: number;
}

/**
 * The bars from the shortest duration that holds a request to the longest,
 * the empty ones between them included, so the axis runs without gaps. A
 * narrow spread is widened on both sides, so a few bars still stand on a
 * scale that says how long they are.
 */
export function latencySlots(bars: readonly TraceRedLatencyBar[]): LatencySlot[] {
  const placed = bars
    .map((bar) => ({ bar, index: bar.fromNs === 0 ? 0 : LATENCY_EDGES_NS.indexOf(bar.fromNs) }))
    .filter(({ index }) => index >= 0);
  if (placed.length === 0) return [];

  let first = Math.min(...placed.map(({ index }) => index));
  let last = Math.max(...placed.map(({ index }) => index));

  while (last - first + 1 < MIN_LATENCY_SLOTS && (first > 0 || last < LATENCY_EDGES_NS.length - 1)) {
    if (first > 0) first -= 1;
    if (last - first + 1 < MIN_LATENCY_SLOTS && last < LATENCY_EDGES_NS.length - 1) last += 1;
  }

  const slots: LatencySlot[] = [];

  for (let index = first; index <= last; index += 1) {
    slots.push({
      fromNs: index === 0 ? 0 : LATENCY_EDGES_NS[index]!,
      toNs: LATENCY_EDGES_NS[index + 1] ?? null,
      ok: 0,
      errors: 0,
    });
  }

  for (const { bar, index } of placed) {
    const slot = slots[index - first]!;
    const errors = Math.min(bar.errors ?? 0, bar.requests);
    slot.errors += errors;
    slot.ok += bar.requests - errors;
  }

  return slots;
}
