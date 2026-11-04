import type { RumAggregate, RumAggregateParams, RumFilter, RumMeasure, RumRow, RumSearchParams } from '@/api/rum';

import { count, inUnit, named, numeric, type OverviewRange, type OverviewScope, percentile, scopeFilters } from '../../overview/model';
import type { Phases } from '../../waterfall/model';
import { cohortFilters } from '../model';

// ---------------------------------------------------------------------------------------------
// What the requests are told apart by, and narrowed by.

const DURATION = 'resource.duration';
const SIZE = 'resource.size';
const STATUS = 'resource.status_code';
const DELIVERY = 'resource.delivery_type';
const TRACE = 'resource.trace_id';

/** The kinds of request an SDK tells apart, the ones that run a page first. */
export const RESOURCE_TYPES = ['document', 'js', 'css', 'image', 'font', 'media', 'fetch', 'xhr', 'beacon', 'other', 'native'] as const;

/** What the table can be of: the groups of requests it lists. A group is the values of one or two columns. */
export const GROUPS = [
  { key: 'host_type', fields: ['resource.url_host', 'resource.type'] },
  { key: 'path', fields: ['resource.url_host', 'resource.url_path_group'] },
  { key: 'type', fields: ['resource.type'] },
  { key: 'protocol', fields: ['resource.protocol'] },
  { key: 'delivery', fields: [DELIVERY] },
  { key: 'blocking', fields: ['resource.render_blocking_status'] },
] as const;

export type Group = (typeof GROUPS)[number];
export type GroupKey = Group['key'];

export const DEFAULT_GROUP: GroupKey = 'host_type';

export const groupOf = (key: GroupKey): Group => GROUPS.find((group) => group.key === key) as Group;

export const parseGroup = (value: string | null): GroupKey => GROUPS.find((group) => group.key === value)?.key ?? DEFAULT_GROUP;

/** The answers worth telling apart: the ones that went wrong, by who is to blame, and none at all. */
export const STATUS_FILTERS = ['4xx', '5xx', 'none'] as const;

export type StatusFilter = '' | (typeof STATUS_FILTERS)[number];

export const parseStatus = (value: string | null): StatusFilter => STATUS_FILTERS.find((status) => status === value) ?? '';

export const parseType = (value: string | null): string => RESOURCE_TYPES.find((type) => type === value) ?? '';

/** What narrows the requests the table and the panel are of: a kind, how they were answered, words in the address. */
export interface Narrowing {
  type: string;
  status: StatusFilter;
  search: string;
}

export function narrowFilters(narrowing: Narrowing): RumFilter[] {
  const filters: RumFilter[] = [];

  if (narrowing.type) filters.push({ field: 'resource.type', op: '=', value: narrowing.type });

  if (narrowing.status === '4xx') {
    filters.push({ field: STATUS, op: '>=', value: '400' }, { field: STATUS, op: '<', value: '500' });
  } else if (narrowing.status === '5xx') {
    filters.push({ field: STATUS, op: '>=', value: '500' });
  } else if (narrowing.status === 'none') {
    filters.push({ field: STATUS, op: '=', value: '0' });
  }

  const words = narrowing.search.trim();

  if (words) filters.push({ field: 'resource.url', op: 'contains', value: words });

  return filters;
}

/** The condition that makes the requests of one group: each column its value, or without one where the group has none. */
export const groupFilters = (group: Group, values: readonly (string | null)[]): RumFilter[] =>
  group.fields.map((field, index): RumFilter => {
    const value = values[index];

    return value === null || value === undefined ? { field, op: 'missing', value: '' } : { field, op: '=', value };
  });

/** A group with fewer requests than this is shown, but a ranking leaves it to the end: a handful says nothing. */
export const MIN_REQUESTS = 30;

/** The most groups the table lists: the ones with the most requests; the reader orders the ones that are there. */
export const GROUP_ROWS = 100;

// ---------------------------------------------------------------------------------------------
// What the page asks.

const read = (scope: OverviewScope, range: OverviewRange, extra: RumFilter[] = []) => ({
  type: 'resources' as const,
  from: range.from,
  to: range.to,
  filters: scopeFilters(scope, extra),
});

const present = (name: string, field: string): RumMeasure => ({ fn: 'count', field, name });

const status = (op: RumFilter['op'], value: string): RumFilter[] => [{ field: STATUS, op, value }];

/** Failed requests are counted in two measures that cannot overlap: an error status, and no answer at all. */
const failures = (): RumMeasure[] => [count('failed_http', status('>=', '400')), count('failed_none', status('=', '0'))];

/** How many requests there were, how long they took, how many failed, and how many bytes each view's came to. */
export function statementQuery(scope: OverviewScope, range: OverviewRange): RumAggregateParams {
  return {
    ...read(scope, range),
    measures: [
      count('requests'),
      percentile('p75', DURATION),
      present('with_status', STATUS),
      ...failures(),
      { fn: 'sum', field: SIZE, name: 'bytes' },
      { fn: 'count_distinct', field: 'view.id', name: 'views' },
    ],
  };
}

/** Each group of requests with how many there were, the 75th percentile of their time and size, and how they ended. */
export function tableQuery(scope: OverviewScope, range: OverviewRange, group: Group, narrowing: Narrowing): RumAggregateParams {
  return {
    ...read(scope, range, narrowFilters(narrowing)),
    groupBy: [...group.fields],
    measures: [
      count('requests'),
      percentile('p75', DURATION),
      present('with_status', STATUS),
      ...failures(),
      percentile('size', SIZE),
      count('cached', [{ field: DELIVERY, op: '=', value: 'cache' }]),
      present('with_delivery', DELIVERY),
      present('traced', TRACE),
    ],
    orderBy: 'requests',
    order: 'desc',
    limit: GROUP_ROWS,
  };
}

// ---------------------------------------------------------------------------------------------
// What the page reads out of the answers.

export interface Statement {
  requests: number;
  /** In milliseconds. */
  p75: number | null;
  failed: number;
  /** How many requests said how they ended. */
  withStatus: number;
  views: number;
  bytes: number;
}

export function readStatement(answer: RumAggregate | undefined): Statement | null {
  const row = answer?.rows[0];

  if (!answer || !row) return null;

  const found = named(answer, row);
  const p75 = found.p75 ?? null;

  return {
    requests: found.requests ?? 0,
    p75: p75 === null ? null : inUnit({ nanoseconds: true }, p75),
    failed: (found.failed_http ?? 0) + (found.failed_none ?? 0),
    withStatus: found.with_status ?? 0,
    views: found.views ?? 0,
    bytes: found.bytes ?? 0,
  };
}

/** The share of the requests that said how they ended, and ended badly; none where none said. */
export const failureRate = (value: { failed: number; withStatus: number }): number | null =>
  value.withStatus > 0 ? Math.min(1, value.failed / value.withStatus) : null;

export interface Row {
  /** What the group is called in an address: its values, as a list. */
  key: string;
  /** The values of the columns the table is of; none where the requests have none. */
  values: Array<string | null>;
  requests: number;
  /** The 75th percentile of the time, in milliseconds, and as the SDK sends it: what the parts are taken around. */
  p75: number | null;
  rawP75: number | null;
  withStatus: number;
  failed: number;
  /** The 75th percentile of the size, in bytes. */
  size: number | null;
  cached: number;
  withDelivery: number;
  traced: number;
}

export const keyOf = (values: readonly (string | null)[]): string => JSON.stringify(values);

export function readRows(answer: RumAggregate | undefined): Row[] {
  if (!answer) return [];

  return answer.rows.flatMap((row) => {
    const found = named(answer, row);

    if (found.requests === null || found.requests === undefined) return [];

    const p75 = found.p75 ?? null;

    return [
      {
        key: keyOf(row.group),
        values: row.group.map((value) => value || null),
        requests: found.requests,
        p75: p75 === null ? null : inUnit({ nanoseconds: true }, p75),
        rawP75: p75,
        withStatus: found.with_status ?? 0,
        failed: (found.failed_http ?? 0) + (found.failed_none ?? 0),
        size: found.size ?? null,
        cached: found.cached ?? 0,
        withDelivery: found.with_delivery ?? 0,
        traced: found.traced ?? 0,
      },
    ];
  });
}

/** The share of the requests that said how they were delivered, and were delivered from the cache. */
export const cacheShare = (row: Row): number | null => (row.withDelivery > 0 ? Math.min(1, row.cached / row.withDelivery) : null);

/** The share of the requests that carried the id of a trace of the backend. */
export const traceShare = (row: Row): number | null => (row.requests > 0 ? Math.min(1, row.traced / row.requests) : null);

// ---------------------------------------------------------------------------------------------
// How the table is ordered.

export type SortKey = 'requests' | 'p75' | 'failed' | 'size' | 'cache';

export const SORTS: readonly SortKey[] = ['requests', 'p75', 'failed', 'size', 'cache'];

export const DEFAULT_SORT: SortKey = 'requests';

export const parseSort = (value: string | null): SortKey => SORTS.find((sort) => sort === value) ?? DEFAULT_SORT;

export type Direction = 'asc' | 'desc';

export const parseDirection = (value: string | null): Direction | undefined => (value === 'asc' || value === 'desc' ? value : undefined);

/** The order a column reads in when none is chosen: the most, the slowest, the most failed, the largest, the least cached first. */
export const defaultDirection = (sort: SortKey): Direction => (sort === 'cache' ? 'asc' : 'desc');

const sortValue = (row: Row, sort: SortKey): number | null => {
  switch (sort) {
    case 'requests':
      return row.requests;
    case 'p75':
      return row.p75;
    case 'failed':
      return failureRate(row);
    case 'size':
      return row.size;
    case 'cache':
      return cacheShare(row);
  }
};

/**
 * The rows in the order a reader chose. A ranking by anything but the number of requests leaves the
 * groups with too few requests to the end, whichever way it runs: a handful of requests is not a slow host.
 */
export function sortRows(rows: readonly Row[], sort: SortKey, direction: Direction): Row[] {
  return [...rows].sort((a, b) => {
    if (sort !== 'requests') {
      const fewA = a.requests < MIN_REQUESTS;
      const fewB = b.requests < MIN_REQUESTS;

      if (fewA !== fewB) return fewA ? 1 : -1;
    }

    const x = sortValue(a, sort);
    const y = sortValue(b, sort);

    if (x === y) return b.requests - a.requests;
    if (x === null) return 1;
    if (y === null) return -1;

    return direction === 'asc' ? x - y : y - x;
  });
}

// ---------------------------------------------------------------------------------------------
// What a group is made of.

const PHASE_FIELDS = [
  ['redirect', 'resource.redirect.duration'],
  ['dns', 'resource.dns.duration'],
  ['connect', 'resource.connect.duration'],
  ['ssl', 'resource.ssl.duration'],
  ['first_byte', 'resource.first_byte.duration'],
  ['download', 'resource.download.duration'],
] as const;

/** What the requests that stand for the 75th percentile of a group took their time on: each part averaged. */
export function phasesQuery(
  scope: OverviewScope,
  range: OverviewRange,
  narrowing: Narrowing,
  group: RumFilter[],
  raw: number,
): RumAggregateParams {
  return {
    ...read(scope, range, [...narrowFilters(narrowing), ...group, ...cohortFilters({ field: DURATION, nanoseconds: true }, raw)]),
    measures: [count('requests'), ...PHASE_FIELDS.map(([name, field]): RumMeasure => ({ fn: 'avg', field, name }))],
  };
}

export interface Makeup {
  /** The requests the parts were taken over. */
  requests: number;
  phases: Phases;
}

/** The parts of the requests around the 75th percentile, in milliseconds; none where no request said any. */
export function readPhases(answer: RumAggregate | undefined): Makeup | null {
  const row = answer?.rows[0];

  if (!answer || !row) return null;

  const found = named(answer, row);
  const part = (name: string): number | null => (found[name] === null || found[name] === undefined ? null : inUnit({ nanoseconds: true }, found[name] as number));
  const connect = part('connect');
  const tls = part('ssl');
  const phases: Phases = {
    redirect: part('redirect'),
    dns: part('dns'),
    // The handshake is part of the connection in the timing the browser reports: told apart, not twice.
    connect: connect === null ? null : Math.max(0, connect - (tls ?? 0)),
    tls,
    first_byte: part('first_byte'),
    download: part('download'),
  };

  return Object.values(phases).every((value) => value === null) ? null : { requests: found.requests ?? 0, phases };
}

export const STATUS_ROWS = 6;

/** How the requests of a group were answered, the commonest first. */
export function statusesQuery(scope: OverviewScope, range: OverviewRange, narrowing: Narrowing, group: RumFilter[]): RumAggregateParams {
  return {
    ...read(scope, range, [...narrowFilters(narrowing), ...group, { field: STATUS, op: 'exists', value: '' }]),
    groupBy: [STATUS],
    measures: [count('requests')],
    orderBy: 'requests',
    order: 'desc',
    limit: STATUS_ROWS,
  };
}

export interface StatusShare {
  status: number;
  requests: number;
}

export function readStatuses(answer: RumAggregate | undefined): StatusShare[] {
  if (!answer) return [];

  return answer.rows.flatMap((row) => {
    const status = Number(row.group[0]);
    const requests = named(answer, row).requests;

    return Number.isFinite(status) && requests !== null && requests !== undefined ? [{ status, requests }] : [];
  });
}

export const SLOWEST_ROWS = 3;

/** The requests of a group that took the longest: which they were, and how long. */
export function slowestQuery(scope: OverviewScope, range: OverviewRange, narrowing: Narrowing, group: RumFilter[]): RumAggregateParams {
  return {
    ...read(scope, range, [...narrowFilters(narrowing), ...group, { field: DURATION, op: 'exists', value: '' }]),
    groupBy: ['resource.id'],
    measures: [{ fn: 'max', field: DURATION, name: 'slowest' }],
    orderBy: 'slowest',
    order: 'desc',
    limit: SLOWEST_ROWS,
  };
}

export interface Slowest {
  id: string;
  /** In milliseconds. */
  ms: number;
}

export function readSlowest(answer: RumAggregate | undefined): Slowest[] {
  if (!answer) return [];

  return answer.rows.flatMap((row) => {
    const id = row.group[0];
    const raw = named(answer, row).slowest;

    return id && raw !== null && raw !== undefined ? [{ id, ms: inUnit({ nanoseconds: true }, raw) }] : [];
  });
}

/** The columns of the slowest requests, to say what they were and where they happened. */
export const SLOWEST_COLUMNS = ['resource.id', 'resource.method', 'resource.url_path', 'resource.status_code', 'view.id', 'view.url_path'] as const;

/** The requests with these ids, in the range, as rows. */
export function slowestRowsQuery(scope: OverviewScope, range: OverviewRange, ids: readonly string[]): RumSearchParams {
  return {
    ...read(scope, range, [{ field: 'resource.id', op: 'in', value: JSON.stringify(ids) }]),
    columns: [...SLOWEST_COLUMNS],
    limit: ids.length,
  };
}

export interface SlowRequest extends Slowest {
  method: string | null;
  path: string;
  status: number | null;
  /** The view it was made by, and when it was made, in microseconds: the view is opened from here. */
  view: string | null;
  at: number | null;
  page: string | null;
}

const text = (row: RumRow | undefined, field: string): string | null => {
  const value = row?.[field];

  return typeof value === 'string' && value.trim() !== '' ? value : null;
};

/** The slowest requests, each with what the row of the request says about it. */
export function slowRequests(slowest: readonly Slowest[], rows: readonly RumRow[]): SlowRequest[] {
  return slowest.map((entry) => {
    const row = rows.find((candidate) => candidate['resource.id'] === entry.id);

    return {
      ...entry,
      method: text(row, 'resource.method'),
      path: text(row, 'resource.url_path') ?? entry.id,
      status: numeric(row?.['resource.status_code']),
      view: text(row, 'view.id'),
      at: numeric(row?.timestamp),
      page: text(row, 'view.url_path'),
    };
  });
}

/** The 75th percentile of the time of a group in each bar of the range; the bars begin where the frame does. */
export function trendQuery(
  scope: OverviewScope,
  bars: { binMicros: number; frame: { start: number; end: number } },
  narrowing: Narrowing,
  group: RumFilter[],
): RumAggregateParams {
  return {
    ...read(scope, { from: bars.frame.start, to: bars.frame.end }, [...narrowFilters(narrowing), ...group]),
    measures: [percentile('p75', DURATION)],
    bucketTime: bars.binMicros,
    originTime: bars.frame.start,
  };
}
