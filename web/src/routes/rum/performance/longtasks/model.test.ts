import { describe, expect, it } from 'vitest';

import type { RumAggregate, RumRow } from '@/api/rum';

import {
  GROUPS,
  groupFilters,
  groupOf,
  invokersQuery,
  keyOf,
  mainPage,
  MIN_TASKS,
  pagesQuery,
  parseGroup,
  parseSort,
  readRows,
  readSamples,
  readShare,
  readShares,
  readStatement,
  sampleTasks,
  samplesQuery,
  samplesRowsQuery,
  sortRows,
  statementQuery,
  tableQuery,
  viewsQuery,
} from './model';

const scope = { application: 'shop', environment: '' };
const range = { from: 1_000, to: 9_000 };
const ms = (value: number) => value * 1_000_000;

const answer = (measures: string[], rows: Array<{ group?: Array<string | null>; values: Array<number | null> }>): RumAggregate => ({
  groupBy: [],
  measures: measures.map((name) => ({ name, fn: 'count' as const, unit: 'count' as const })),
  rows: rows.map((row) => ({ group: row.group ?? [], values: row.values })),
});

describe('what the table can be of', () => {
  it('reads a group from an address, and falls back to the scripts', () => {
    expect(parseGroup('page')).toBe('page');
    expect(parseGroup('x')).toBe('script');
    expect(parseGroup(null)).toBe('script');
    expect(parseSort('blocking')).toBe('blocking');
    expect(parseSort('x')).toBe('tasks');
  });

  it('tells a script by its address and the function in it, and any other group by one column', () => {
    expect(groupOf('script').fields).toEqual(['long_task.top_script.source_url', 'long_task.top_script.function']);

    for (const group of GROUPS) expect(group.fields.length).toBeLessThanOrEqual(2);
  });

  it('makes the long tasks of a group by the values of its columns, and by the lack of one', () => {
    expect(groupFilters(groupOf('script'), ['/app.js', null])).toEqual([
      { field: 'long_task.top_script.source_url', op: '=', value: '/app.js' },
      { field: 'long_task.top_script.function', op: 'missing', value: '' },
    ]);
  });
});

describe('the reads', () => {
  it('asks how many long tasks there were and how long they ran and blocked', () => {
    const asked = statementQuery(scope, range);

    expect(asked.type).toBe('long_tasks');
    expect(asked.measures.map((measure) => measure.name)).toEqual(['tasks', 'p75', 'blocking']);
    expect(asked.filters).toEqual([{ field: 'application.id', op: '=', value: 'shop' }]);
  });

  it('asks how many views met a long task, from the views', () => {
    const asked = viewsQuery(scope, range);

    expect(asked.type).toBe('views');
    expect(asked.measures[1]).toMatchObject({ name: 'with_tasks', where: [{ field: 'view.long_task.count', op: '>', value: '0' }] });
  });

  it('asks for the groups with the most long tasks first, narrowed by words of the script', () => {
    const asked = tableQuery(scope, range, groupOf('script'), ' checkout ');

    expect(asked).toMatchObject({ groupBy: ['long_task.top_script.source_url', 'long_task.top_script.function'], orderBy: 'tasks', order: 'desc', limit: 100 });
    expect(asked.filters).toContainEqual({ field: 'long_task.top_script.source_url', op: 'contains', value: 'checkout' });
    expect(tableQuery(scope, range, groupOf('page'), '').filters).toHaveLength(1);
  });

  it('asks the shares of a group by their invoker and by their page, narrowed as the table is', () => {
    const group = groupFilters(groupOf('script'), ['/app.js', 'render']);

    expect(invokersQuery(scope, range, group, '')).toMatchObject({ groupBy: ['long_task.top_script.invoker_type'], limit: 4 });
    expect(pagesQuery(scope, range, group, 'app').filters).toContainEqual({ field: 'long_task.top_script.source_url', op: 'contains', value: 'app' });
  });

  it('asks for the longest three long tasks of a group, and then for their rows', () => {
    expect(samplesQuery(scope, range, [], '')).toMatchObject({ groupBy: ['long_task.id'], orderBy: 'longest', order: 'desc', limit: 3 });

    const rows = samplesRowsQuery(scope, range, ['a']);

    expect(rows.filters).toContainEqual({ field: 'long_task.id', op: 'in', value: '["a"]' });
    expect(rows.columns).toContain('view.id');
  });
});

describe('what the reads say', () => {
  it('reads the statement in milliseconds, and the share of the views', () => {
    expect(readStatement(answer(['tasks', 'p75', 'blocking'], [{ values: [468_000, ms(118), ms(64)] }]))).toEqual({ tasks: 468_000, p75: 118, blocking: 64 });
    expect(readStatement(answer(['tasks'], []))).toBeNull();
    expect(readShare(answer(['views', 'with_tasks'], [{ values: [1_000, 620] }]))).toBe(0.62);
    expect(readShare(answer(['views', 'with_tasks'], [{ values: [0, 0] }]))).toBeNull();
    expect(readShare(undefined)).toBeNull();
  });

  const names = ['tasks', 'views', 'p75', 'blocking'];

  it('reads the rows of the table, the unattributed one with no script', () => {
    const rows = readRows(
      answer(names, [
        { group: ['/assets/checkout.js', 'validateAddress'], values: [39_000, 22_000, ms(210), ms(158)] },
        { group: [null, null], values: [99_000, 43_000, ms(74), null] },
      ]),
    );

    expect(rows[0]).toEqual({ key: keyOf(['/assets/checkout.js', 'validateAddress']), values: ['/assets/checkout.js', 'validateAddress'], tasks: 39_000, views: 22_000, p75: 210, blocking: 158 });
    expect(rows[1]).toMatchObject({ values: [null, null], blocking: null });
  });

  it('orders by a column, with the groups of too few long tasks, and the ones with no value, at the end', () => {
    const rows = readRows(
      answer(names, [
        { group: ['a', null], values: [1_000, 500, ms(100), ms(10)] },
        { group: ['b', null], values: [2_000, 900, ms(300), ms(200)] },
        { group: ['c', null], values: [MIN_TASKS - 1, 5, ms(9_000), ms(9_000)] },
        { group: ['d', null], values: [800, 100, null, null] },
      ]),
    );
    const order = (sort: Parameters<typeof sortRows>[1], direction: 'asc' | 'desc') => sortRows(rows, sort, direction).map((row) => row.values[0]);

    expect(order('tasks', 'desc')).toEqual(['b', 'a', 'd', 'c']);
    expect(order('p75', 'desc')).toEqual(['b', 'a', 'd', 'c']);
    expect(order('p75', 'asc')).toEqual(['a', 'b', 'd', 'c']);
    expect(order('blocking', 'desc')).toEqual(['b', 'a', 'd', 'c']);
  });

  it('reads the shares, the samples, and the page most of the long tasks were on', () => {
    const shares = readShares(answer(['tasks'], [{ group: ['/checkout'], values: [30] }, { group: ['/cart'], values: [6] }, { group: [null], values: [4] }]));

    expect(shares).toEqual([
      { value: '/checkout', tasks: 30 },
      { value: '/cart', tasks: 6 },
      { value: null, tasks: 4 },
    ]);
    expect(mainPage(shares, 40)).toBe('/checkout');
    expect(mainPage(shares, 100)).toBeNull();
    expect(mainPage([], 0)).toBeNull();

    const samples = readSamples(answer(['longest'], [{ group: ['t-1'], values: [ms(412)] }, { group: [null], values: [ms(5)] }]));

    expect(samples).toEqual([{ id: 't-1', ms: 412 }]);

    const rows: RumRow[] = [{ timestamp: 7_000, 'long_task.id': 't-1', 'view.id': 'v-1', 'view.url_path': '/checkout' }];

    expect(sampleTasks(samples, rows)).toEqual([{ id: 't-1', ms: 412, view: 'v-1', at: 7_000, page: '/checkout' }]);
    expect(sampleTasks(samples, [])).toEqual([{ id: 't-1', ms: 412, view: null, at: null, page: null }]);
  });
});
