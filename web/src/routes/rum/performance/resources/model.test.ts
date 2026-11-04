import { describe, expect, it } from 'vitest';

import type { RumAggregate, RumRow } from '@/api/rum';

import {
  cacheShare,
  failureRate,
  GROUPS,
  groupFilters,
  groupOf,
  keyOf,
  MIN_REQUESTS,
  narrowFilters,
  parseGroup,
  parseSort,
  parseStatus,
  parseType,
  phasesQuery,
  readPhases,
  readRows,
  readSlowest,
  readStatement,
  readStatuses,
  type Row,
  slowRequests,
  slowestQuery,
  slowestRowsQuery,
  sortRows,
  statementQuery,
  statusesQuery,
  tableQuery,
  trendQuery,
} from './model';

const scope = { application: 'shop', environment: 'prod' };
const range = { from: 1_000, to: 9_000 };

const answer = (measures: string[], rows: Array<{ group?: Array<string | null>; values: Array<number | null> }>): RumAggregate => ({
  groupBy: [],
  measures: measures.map((name) => ({ name, fn: 'count' as const, unit: 'count' as const })),
  rows: rows.map((row) => ({ group: row.group ?? [], values: row.values })),
});

const ms = (value: number) => value * 1_000_000;

describe('what the table can be of', () => {
  it('reads a group and a status from an address, and falls back to the first group', () => {
    expect(parseGroup('path')).toBe('path');
    expect(parseGroup('nope')).toBe('host_type');
    expect(parseGroup(null)).toBe('host_type');
    expect(parseStatus('5xx')).toBe('5xx');
    expect(parseStatus('200')).toBe('');
    expect(parseType('image')).toBe('image');
    expect(parseType('video')).toBe('');
    expect(parseSort('p75')).toBe('p75');
    expect(parseSort('x')).toBe('requests');
  });

  it('tells a group by at most two columns, as the aggregate allows', () => {
    for (const group of GROUPS) expect(group.fields.length).toBeLessThanOrEqual(2);
  });

  it('narrows by kind, by how the requests were answered, and by the words of their address', () => {
    expect(narrowFilters({ type: '', status: '', search: '' })).toEqual([]);
    expect(narrowFilters({ type: 'fetch', status: '', search: ' /api ' })).toEqual([
      { field: 'resource.type', op: '=', value: 'fetch' },
      { field: 'resource.url', op: 'contains', value: '/api' },
    ]);
    expect(narrowFilters({ type: '', status: '4xx', search: '' })).toEqual([
      { field: 'resource.status_code', op: '>=', value: '400' },
      { field: 'resource.status_code', op: '<', value: '500' },
    ]);
    expect(narrowFilters({ type: '', status: '5xx', search: '' })).toEqual([{ field: 'resource.status_code', op: '>=', value: '500' }]);
    expect(narrowFilters({ type: '', status: 'none', search: '' })).toEqual([{ field: 'resource.status_code', op: '=', value: '0' }]);
  });

  it('makes the requests of a group by the values of its columns, and by the lack of one', () => {
    expect(groupFilters(groupOf('host_type'), ['api.example.com', 'fetch'])).toEqual([
      { field: 'resource.url_host', op: '=', value: 'api.example.com' },
      { field: 'resource.type', op: '=', value: 'fetch' },
    ]);
    expect(groupFilters(groupOf('protocol'), [null])).toEqual([{ field: 'resource.protocol', op: 'missing', value: '' }]);
  });
});

describe('the reads', () => {
  const narrowing = { type: 'js', status: '' as const, search: '' };

  it('asks for the requests of the application in the environment', () => {
    const asked = statementQuery(scope, range);

    expect(asked.type).toBe('resources');
    expect(asked.filters).toEqual([
      { field: 'application.id', op: '=', value: 'shop' },
      { field: 'env', op: '=', value: 'prod' },
    ]);
    expect(asked.measures.map((measure) => measure.name)).toEqual(['requests', 'p75', 'with_status', 'failed_http', 'failed_none', 'bytes', 'views']);
  });

  it('asks for the groups the most requested first, within what an answer can hold', () => {
    const asked = tableQuery(scope, range, groupOf('host_type'), narrowing);

    expect(asked.groupBy).toEqual(['resource.url_host', 'resource.type']);
    expect(asked.measures.length).toBeLessThanOrEqual(16);
    expect(asked).toMatchObject({ orderBy: 'requests', order: 'desc', limit: 100 });
    expect(asked.filters).toContainEqual({ field: 'resource.type', op: '=', value: 'js' });
  });

  it('counts the failed in two measures that cannot overlap', () => {
    const measures = statementQuery(scope, range).measures;

    expect(measures.find((measure) => measure.name === 'failed_http')?.where).toEqual([{ field: 'resource.status_code', op: '>=', value: '400' }]);
    expect(measures.find((measure) => measure.name === 'failed_none')?.where).toEqual([{ field: 'resource.status_code', op: '=', value: '0' }]);
  });

  it('takes the parts of the requests that stand for the 75th percentile, narrowed as the table is', () => {
    const group = groupFilters(groupOf('host_type'), ['api.example.com', 'fetch']);
    const asked = phasesQuery(scope, range, narrowing, group, ms(380));

    expect(asked.filters).toEqual(
      expect.arrayContaining([
        { field: 'resource.url_host', op: '=', value: 'api.example.com' },
        { field: 'resource.type', op: '=', value: 'js' },
        { field: 'resource.duration', op: '>=', value: String(ms(342)) },
        { field: 'resource.duration', op: '<=', value: String(ms(418)) },
      ]),
    );
    expect(asked.measures.map((measure) => measure.name)).toEqual(['requests', 'redirect', 'dns', 'connect', 'ssl', 'first_byte', 'download']);
  });

  it('asks how a group was answered, leaving out the requests that did not say', () => {
    const asked = statusesQuery(scope, range, narrowing, groupFilters(groupOf('type'), ['js']));

    expect(asked.groupBy).toEqual(['resource.status_code']);
    expect(asked.filters).toContainEqual({ field: 'resource.status_code', op: 'exists', value: '' });
  });

  it('asks for the slowest three of a group, and then for their rows', () => {
    const asked = slowestQuery(scope, range, narrowing, []);

    expect(asked).toMatchObject({ groupBy: ['resource.id'], orderBy: 'slowest', order: 'desc', limit: 3 });

    const rows = slowestRowsQuery(scope, range, ['a', 'b']);

    expect(rows.filters).toContainEqual({ field: 'resource.id', op: 'in', value: '["a","b"]' });
    expect(rows.limit).toBe(2);
  });

  it('follows a group through the bars of the range', () => {
    const bars = { binMicros: 100, frame: { start: 1_000, end: 2_000 } };
    const asked = trendQuery(scope, bars, narrowing, []);

    expect(asked).toMatchObject({ bucketTime: 100, originTime: 1_000, from: 1_000, to: 2_000 });
  });
});

describe('what the reads say', () => {
  it('reads the statement, counting the failed from both measures', () => {
    const found = readStatement(
      answer(['requests', 'p75', 'with_status', 'failed_http', 'failed_none', 'bytes', 'views'], [{ values: [8_200, ms(420), 8_000, 150, 20, 4_000_000, 100] }]),
    );

    expect(found).toEqual({ requests: 8_200, p75: 420, failed: 170, withStatus: 8_000, views: 100, bytes: 4_000_000 });
    expect(failureRate(found as NonNullable<typeof found>)).toBeCloseTo(0.02125);
  });

  it('has no statement and no failure rate where nothing was answered', () => {
    expect(readStatement(answer(['requests'], []))).toBeNull();
    expect(failureRate({ failed: 3, withStatus: 0 })).toBeNull();
    expect(failureRate({ failed: 5, withStatus: 2 })).toBe(1);
  });

  const names = ['requests', 'p75', 'with_status', 'failed_http', 'failed_none', 'size', 'cached', 'with_delivery', 'traced'];

  it('reads the rows of the table, each named by its values', () => {
    const rows = readRows(
      answer(names, [
        { group: ['cdn.example.com', 'image'], values: [3_120, ms(640), 3_100, 9, 0, 118_000, 820, 1_000, 0] },
        { group: ['api.example.com', null], values: [1_480, ms(380), 1_480, 70, 1, 4_000, 0, 0, 1_050] },
      ]),
    );

    expect(rows).toHaveLength(2);
    expect(rows[0]).toMatchObject({ key: keyOf(['cdn.example.com', 'image']), values: ['cdn.example.com', 'image'], requests: 3_120, p75: 640, rawP75: ms(640), failed: 9, size: 118_000 });
    expect(rows[1]).toMatchObject({ values: ['api.example.com', null], failed: 71 });
    expect(cacheShare(rows[0] as Row)).toBe(0.82);
    expect(cacheShare(rows[1] as Row)).toBeNull();
  });

  it('orders by a column, and leaves the groups with too few requests to the end whichever way', () => {
    const rows = readRows(
      answer(names, [
        { group: ['a', 'js'], values: [1_000, ms(100), 1_000, 10, 0, 1, 0, 0, 0] },
        { group: ['b', 'js'], values: [2_000, ms(300), 2_000, 100, 0, 1, 0, 0, 0] },
        { group: ['c', 'js'], values: [MIN_REQUESTS - 1, ms(9_000), 5, 5, 0, 1, 0, 0, 0] },
        { group: ['d', 'js'], values: [500, null, 0, 0, 0, null, 0, 0, 0] },
      ]),
    );
    const order = (sort: Parameters<typeof sortRows>[1], direction: 'asc' | 'desc') => sortRows(rows, sort, direction).map((row) => row.values[0]);

    expect(order('requests', 'desc')).toEqual(['b', 'a', 'd', 'c']);
    expect(order('p75', 'desc')).toEqual(['b', 'a', 'd', 'c']);
    expect(order('p75', 'asc')).toEqual(['a', 'b', 'd', 'c']);
    expect(order('failed', 'desc')).toEqual(['b', 'a', 'd', 'c']);
  });

  it('reads the parts of a request, the handshake apart from the connection', () => {
    const made = readPhases(
      answer(
        ['requests', 'redirect', 'dns', 'connect', 'ssl', 'first_byte', 'download'],
        [{ values: [40, null, ms(6), ms(32), ms(18), ms(210), ms(132)] }],
      ),
    );

    expect(made?.requests).toBe(40);
    expect(made?.phases).toEqual({ redirect: null, dns: 6, connect: 14, tls: 18, first_byte: 210, download: 132 });
  });

  it('has no parts where no request says any', () => {
    expect(readPhases(answer(['requests', 'redirect', 'dns', 'connect', 'ssl', 'first_byte', 'download'], [{ values: [4, null, null, null, null, null, null] }]))).toBeNull();
    expect(readPhases(answer(['requests'], []))).toBeNull();
  });

  it('reads the statuses and the slowest, and joins the slowest to their rows', () => {
    expect(readStatuses(answer(['requests'], [{ group: ['200'], values: [926] }, { group: ['500'], values: [23] }, { group: ['x'], values: [1] }]))).toEqual([
      { status: 200, requests: 926 },
      { status: 500, requests: 23 },
    ]);

    const slowest = readSlowest(answer(['slowest'], [{ group: ['r-1'], values: [ms(4_200)] }, { group: ['r-2'], values: [ms(3_800)] }]));

    expect(slowest).toEqual([
      { id: 'r-1', ms: 4_200 },
      { id: 'r-2', ms: 3_800 },
    ]);

    const rows: RumRow[] = [
      {
        timestamp: 5_000,
        'resource.id': 'r-2',
        'resource.method': 'POST',
        'resource.url_path': '/v2/checkout/confirm',
        'resource.status_code': 500,
        'view.id': 'v-9',
        'view.url_path': '/checkout',
      },
    ];

    expect(slowRequests(slowest, rows)).toEqual([
      { id: 'r-1', ms: 4_200, method: null, path: 'r-1', status: null, view: null, at: null, page: null },
      { id: 'r-2', ms: 3_800, method: 'POST', path: '/v2/checkout/confirm', status: 500, view: 'v-9', at: 5_000, page: '/checkout' },
    ]);
  });
});
