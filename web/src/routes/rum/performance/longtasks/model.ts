import type { RumAggregate, RumAggregateParams, RumFilter, RumRow, RumSearchParams } from '@/api/rum';

import { count, inUnit, named, numeric, type OverviewRange, type OverviewScope, percentile, scopeFilters } from '../../overview/model';

// ---------------------------------------------------------------------------------------------
// What the long tasks are told apart by.

const DURATION = 'long_task.duration';
const BLOCKING = 'long_task.blocking_duration';
const SCRIPT = 'long_task.top_script.source_url';
const FUNCTION = 'long_task.top_script.function';
const INVOKER = 'long_task.top_script.invoker_type';
const PAGE = 'view.url_path_group';

/** What the table can be of: the groups of long tasks it lists. A group is the values of one or two columns. */
export const GROUPS = [
  { key: 'script', fields: [SCRIPT, FUNCTION] },
  { key: 'page', fields: [PAGE] },
  { key: 'version', fields: ['version'] },
  { key: 'invoker', fields: [INVOKER] },
] as const;

export type Group = (typeof GROUPS)[number];
export type GroupKey = Group['key'];

export const DEFAULT_GROUP: GroupKey = 'script';

export const groupOf = (key: GroupKey): Group => GROUPS.find((group) => group.key === key) as Group;

export const parseGroup = (value: string | null): GroupKey => GROUPS.find((group) => group.key === value)?.key ?? DEFAULT_GROUP;

/** The condition that makes the long tasks of one group: each column its value, or without one where the group has none. */
export const groupFilters = (group: Group, values: readonly (string | null)[]): RumFilter[] =>
  group.fields.map((field, index): RumFilter => {
    const value = values[index];

    return value === null || value === undefined ? { field, op: 'missing', value: '' } : { field, op: '=', value };
  });

/** A group with fewer long tasks than this is shown, but a ranking leaves it to the end. */
export const MIN_TASKS = 30;

/** The most groups the table lists: the ones with the most long tasks; the reader orders the ones that are there. */
export const GROUP_ROWS = 100;

/** From how long a long task is worth an orange word, and from how long it is a red one. */
export const SLOW_MS = 100;
export const VERY_SLOW_MS = 200;

// ---------------------------------------------------------------------------------------------
// What the page asks.

const read = (scope: OverviewScope, range: OverviewRange, extra: RumFilter[] = []) => ({
  type: 'long_tasks' as const,
  from: range.from,
  to: range.to,
  filters: scopeFilters(scope, extra),
});

/** How many long tasks there were, and how long they ran and held up the input at the 75th percentile. */
export function statementQuery(scope: OverviewScope, range: OverviewRange): RumAggregateParams {
  return {
    ...read(scope, range),
    measures: [count('tasks'), percentile('p75', DURATION), percentile('blocking', BLOCKING)],
  };
}

/** How many page views there were, and how many of them met a long task. */
export function viewsQuery(scope: OverviewScope, range: OverviewRange): RumAggregateParams {
  return {
    type: 'views',
    from: range.from,
    to: range.to,
    filters: scopeFilters(scope),
    measures: [count('views'), count('with_tasks', [{ field: 'view.long_task.count', op: '>', value: '0' }])],
  };
}

/** Each group of long tasks with how many there were, in how many views, and how long they ran and blocked. */
export function tableQuery(scope: OverviewScope, range: OverviewRange, group: Group, search: string): RumAggregateParams {
  const words = search.trim();

  return {
    ...read(scope, range, words ? [{ field: SCRIPT, op: 'contains', value: words }] : []),
    groupBy: [...group.fields],
    measures: [count('tasks'), { fn: 'count_distinct', field: 'view.id', name: 'views' }, percentile('p75', DURATION), percentile('blocking', BLOCKING)],
    orderBy: 'tasks',
    order: 'desc',
    limit: GROUP_ROWS,
  };
}

// ---------------------------------------------------------------------------------------------
// What the page reads out of the answers.

export interface Statement {
  tasks: number;
  /** In milliseconds. */
  p75: number | null;
  blocking: number | null;
}

export function readStatement(answer: RumAggregate | undefined): Statement | null {
  const row = answer?.rows[0];

  if (!answer || !row) return null;

  const found = named(answer, row);
  const ms = (raw: number | null | undefined) => (raw === null || raw === undefined ? null : inUnit({ nanoseconds: true }, raw));

  return { tasks: found.tasks ?? 0, p75: ms(found.p75), blocking: ms(found.blocking) };
}

/** The share of the page views that met at least one long task; none where there were no views. */
export function readShare(answer: RumAggregate | undefined): number | null {
  const row = answer?.rows[0];

  if (!answer || !row) return null;

  const found = named(answer, row);

  return found.views !== null && found.views !== undefined && found.views > 0 ? Math.min(1, (found.with_tasks ?? 0) / found.views) : null;
}

export interface Row {
  /** What the group is called in an address: its values, as a list. */
  key: string;
  /** The values of the columns the table is of; none where the long tasks have none. */
  values: Array<string | null>;
  tasks: number;
  views: number;
  /** In milliseconds. */
  p75: number | null;
  blocking: number | null;
}

export const keyOf = (values: readonly (string | null)[]): string => JSON.stringify(values);

export function readRows(answer: RumAggregate | undefined): Row[] {
  if (!answer) return [];

  return answer.rows.flatMap((row) => {
    const found = named(answer, row);

    if (found.tasks === null || found.tasks === undefined) return [];

    const ms = (raw: number | null | undefined) => (raw === null || raw === undefined ? null : inUnit({ nanoseconds: true }, raw));

    return [
      {
        key: keyOf(row.group),
        values: row.group.map((value) => value || null),
        tasks: found.tasks,
        views: found.views ?? 0,
        p75: ms(found.p75),
        blocking: ms(found.blocking),
      },
    ];
  });
}

// ---------------------------------------------------------------------------------------------
// How the table is ordered.

export type SortKey = 'tasks' | 'views' | 'p75' | 'blocking';

export const SORTS: readonly SortKey[] = ['tasks', 'views', 'p75', 'blocking'];

export const DEFAULT_SORT: SortKey = 'tasks';

export const parseSort = (value: string | null): SortKey => SORTS.find((sort) => sort === value) ?? DEFAULT_SORT;

export type Direction = 'asc' | 'desc';

export const parseDirection = (value: string | null): Direction | undefined => (value === 'asc' || value === 'desc' ? value : undefined);

/** The order a column reads in when none is chosen: the most, the longest first. */
export const defaultDirection = (): Direction => 'desc';

/**
 * The rows in the order a reader chose. A ranking by anything but the number of long tasks leaves the
 * groups with too few of them to the end, whichever way it runs.
 */
export function sortRows(rows: readonly Row[], sort: SortKey, direction: Direction): Row[] {
  return [...rows].sort((a, b) => {
    if (sort !== 'tasks') {
      const fewA = a.tasks < MIN_TASKS;
      const fewB = b.tasks < MIN_TASKS;

      if (fewA !== fewB) return fewA ? 1 : -1;
    }

    const x = a[sort];
    const y = b[sort];

    if (x === y) return b.tasks - a.tasks;
    if (x === null) return 1;
    if (y === null) return -1;

    return direction === 'asc' ? x - y : y - x;
  });
}

// ---------------------------------------------------------------------------------------------
// What a group is made of.

export const SHARE_ROWS = 4;

/** The commonest values of a column among the long tasks of a group, with how many long tasks had each. */
export function sharesQuery(scope: OverviewScope, range: OverviewRange, group: RumFilter[], field: string, search: string): RumAggregateParams {
  const words = search.trim();

  return {
    ...read(scope, range, [...group, ...(words ? [{ field: SCRIPT, op: 'contains' as const, value: words }] : [])]),
    groupBy: [field],
    measures: [count('tasks')],
    orderBy: 'tasks',
    order: 'desc',
    limit: SHARE_ROWS,
  };
}

export const invokersQuery = (scope: OverviewScope, range: OverviewRange, group: RumFilter[], search: string) =>
  sharesQuery(scope, range, group, INVOKER, search);

export const pagesQuery = (scope: OverviewScope, range: OverviewRange, group: RumFilter[], search: string) =>
  sharesQuery(scope, range, group, PAGE, search);

export interface Share {
  /** The value; none where the long tasks have none. */
  value: string | null;
  tasks: number;
}

export function readShares(answer: RumAggregate | undefined): Share[] {
  if (!answer) return [];

  return answer.rows.flatMap((row) => {
    const tasks = named(answer, row).tasks;

    return tasks === null || tasks === undefined ? [] : [{ value: row.group[0] || null, tasks }];
  });
}

export const SAMPLE_ROWS = 3;

/** The longest long tasks of a group: which they were, and how long. */
export function samplesQuery(scope: OverviewScope, range: OverviewRange, group: RumFilter[], search: string): RumAggregateParams {
  const words = search.trim();

  return {
    ...read(scope, range, [...group, ...(words ? [{ field: SCRIPT, op: 'contains' as const, value: words }] : [])]),
    groupBy: ['long_task.id'],
    measures: [{ fn: 'max', field: DURATION, name: 'longest' }],
    orderBy: 'longest',
    order: 'desc',
    limit: SAMPLE_ROWS,
  };
}

export interface Sample {
  id: string;
  /** In milliseconds. */
  ms: number;
}

export function readSamples(answer: RumAggregate | undefined): Sample[] {
  if (!answer) return [];

  return answer.rows.flatMap((row) => {
    const id = row.group[0];
    const raw = named(answer, row).longest;

    return id && raw !== null && raw !== undefined ? [{ id, ms: inUnit({ nanoseconds: true }, raw) }] : [];
  });
}

/** The columns of the longest long tasks, to say where and when they happened. */
export const SAMPLE_COLUMNS = ['long_task.id', 'view.id', 'view.url_path'] as const;

/** The long tasks with these ids, in the range, as rows. */
export function samplesRowsQuery(scope: OverviewScope, range: OverviewRange, ids: readonly string[]): RumSearchParams {
  return {
    ...read(scope, range, [{ field: 'long_task.id', op: 'in', value: JSON.stringify(ids) }]),
    columns: [...SAMPLE_COLUMNS],
    limit: ids.length,
  };
}

export interface SampleTask extends Sample {
  /** The view it blocked, and when it ran, in microseconds: the view is opened from here. */
  view: string | null;
  at: number | null;
  page: string | null;
}

const text = (row: RumRow | undefined, field: string): string | null => {
  const value = row?.[field];

  return typeof value === 'string' && value.trim() !== '' ? value : null;
};

/** The longest long tasks, each with what the row of the long task says about it. */
export function sampleTasks(samples: readonly Sample[], rows: readonly RumRow[]): SampleTask[] {
  return samples.map((sample) => {
    const row = rows.find((candidate) => candidate['long_task.id'] === sample.id);

    return { ...sample, view: text(row, 'view.id'), at: numeric(row?.timestamp), page: text(row, 'view.url_path') };
  });
}

/** The page most of the long tasks of a group were on, when it was most of them. */
export function mainPage(shares: readonly Share[], total: number): string | null {
  const top = shares.find((share) => share.value !== null);

  return top && top.value !== null && total > 0 && top.tasks / total >= 0.5 ? top.value : null;
}
