import { describe, expect, it } from 'vitest';

import type { RumAggregate } from '@/api/rum';

import {
  DIMENSIONS,
  defaultOrder,
  formatAgo,
  parseFilter,
  parseOrder,
  parseSort,
  peopleQueries,
  pickedFilters,
  readPeople,
  readSpread,
  readTrend,
  spreadQuery,
  TREND_DAYS,
  trendQuery,
  trendRange,
  versionSpan,
  whereQuery,
} from './model';

const SCOPE = { application: 'shop-web', environment: 'prod' };
const RANGE = { from: 1_000_000, to: 9_000_000 };
const DAY = 24 * 60 * 60 * 1_000_000;

const answer = (rows: RumAggregate['rows']): RumAggregate => ({ groupBy: [], measures: [], rows });

describe('what the address says', () => {
  it('is the unresolved issues, by sessions, until it says otherwise', () => {
    expect(parseFilter(null)).toBe('unresolved');
    expect(parseFilter('regressed')).toBe('regressed');
    expect(parseFilter('closed')).toBe('unresolved');
    expect(parseSort(null)).toBe('sessions');
    expect(parseSort('last_seen')).toBe('last_seen');
    expect(parseSort('name')).toBe('sessions');
    expect(parseOrder('asc')).toBe('asc');
    expect(parseOrder('up')).toBeUndefined();
  });

  it('orders the biggest and the latest first, and the states by what needs somebody', () => {
    expect(defaultOrder('sessions')).toBe('desc');
    expect(defaultOrder('last_seen')).toBe('desc');
    expect(defaultOrder('status')).toBe('asc');
  });
});

describe('the distributions above the list', () => {
  const version = DIMENSIONS[0];

  it('count the sessions of the listed issues in the application and the environment, over what is picked', () => {
    const query = spreadQuery(SCOPE, RANGE, ['a1', 'b2'], { page: '/checkout' }, version);

    expect(query.type).toBe('errors');
    expect(query.groupBy).toEqual(['version']);
    expect(query.filters).toEqual([
      { field: 'application.id', op: '=', value: 'shop-web' },
      { field: 'env', op: '=', value: 'prod' },
      { field: 'error.issue_id', op: 'in', value: '["a1","b2"]' },
      { field: 'view.url_path_group', op: '=', value: '/checkout' },
    ]);
    expect(query.measures).toEqual([{ fn: 'count_distinct', field: 'session.id', name: 'sessions' }]);
  });

  it('carry the source the list is narrowed to', () => {
    const query = spreadQuery(SCOPE, RANGE, ['a1'], {}, version, [{ field: 'source', op: '=', value: 'browser' }]);

    expect(query.filters).toContainEqual({ field: 'source', op: '=', value: 'browser' });
  });

  it('are read as the commonest value first, and what has no value is left out', () => {
    const spread = readSpread(
      answer([
        { group: ['2.4.1'], values: [12] },
        { group: [null], values: [3] },
        { group: ['2.4.0'], values: ['masked'] },
        { group: ['2.3.2'], values: [5] },
      ]),
    );

    expect(spread).toEqual([
      { value: '2.4.1', sessions: 12 },
      { value: '2.3.2', sessions: 5 },
    ]);
    expect(readSpread(undefined)).toEqual([]);
  });

  it('turn what is picked into the filters of the errors', () => {
    expect(pickedFilters({})).toEqual([]);
    expect(pickedFilters({ version: '2.4.1', browser: 'Chrome' })).toEqual([
      { field: 'version', op: '=', value: '2.4.1' },
      { field: 'browser.name', op: '=', value: 'Chrome' },
    ]);
  });
});

describe('the trend of an issue', () => {
  it('is asked by version and by day over whole local days, today the last', () => {
    const now = new Date(2026, 9, 22, 15, 30);
    const range = trendRange(now);

    expect(new Date(range.from / 1_000).getHours()).toBe(0);
    expect(range.to).toBe(now.getTime() * 1_000);
    expect(Math.round((range.to - range.from) / DAY)).toBe(TREND_DAYS);

    const query = trendQuery(SCOPE, range, 'abc');

    expect(query.groupBy).toEqual(['version']);
    expect(query.bucketTime).toBe(DAY);
    expect(query.originTime).toBe(range.from);
    expect(query.filters).toContainEqual({ field: 'error.issue_id', op: '=', value: 'abc' });
  });

  it('stacks the versions in the order they began, and draws the least frequent as one', () => {
    const range = { from: 0, to: TREND_DAYS * DAY };
    const row = (version: string, day: number, count: number) => ({ group: [version], startTime: day * DAY, values: [count] });

    // Seven versions: v1 began first, v7 last; v6 and v7 are the least frequent.
    const trend = readTrend(
      answer([
        row('v1', 0, 10),
        row('v2', 1, 9),
        row('v3', 2, 8),
        row('v4', 3, 7),
        row('v5', 4, 6),
        row('v6', 5, 2),
        row('v7', 6, 1),
        row('v1', 7, 1),
      ]),
      range,
    );

    expect(trend?.days).toHaveLength(TREND_DAYS);
    // The merged rest comes first, at the foot of a bar; then the versions as they began.
    expect(trend?.series.map((series) => series.version)).toEqual(['', 'v1', 'v2', 'v3', 'v4', 'v5']);
    expect(trend?.series[0]?.counts[5]).toBe(2);
    expect(trend?.series[0]?.counts[6]).toBe(1);
    expect(trend?.series[1]?.counts[0]).toBe(10);
    expect(trend?.series[1]?.counts[7]).toBe(1);
  });

  it('is nothing when it cannot be counted, and ignores bars outside the days', () => {
    expect(readTrend(undefined, RANGE)).toBeNull();
    expect(readTrend({ ...answer([]), unavailable: 'time_budget' }, RANGE)).toBeNull();

    const range = { from: 0, to: TREND_DAYS * DAY };
    const trend = readTrend(answer([{ group: ['v1'], startTime: 99 * DAY, values: [4] }]), range);

    expect(trend?.series).toEqual([]);
  });
});

describe('the drawer of an issue', () => {
  it('counts only the errors of the issue, spread over a field', () => {
    const query = whereQuery(SCOPE, RANGE, 'abc', 'browser.name');

    expect(query.groupBy).toEqual(['browser.name']);
    expect(query.filters).toContainEqual({ field: 'error.issue_id', op: '=', value: 'abc' });
  });

  it('names the people with an id and the visitors without one apart, the most often first', () => {
    const { known, anonymous } = peopleQueries(SCOPE, RANGE, 'abc');

    expect(known.groupBy).toEqual(['usr.id']);
    expect(known.filters).toContainEqual({ field: 'usr.id', op: 'exists', value: '' });
    expect(anonymous.groupBy).toEqual(['usr.anonymous_id']);
    expect(anonymous.filters).toContainEqual({ field: 'usr.id', op: 'missing', value: '' });

    const people = readPeople(
      answer([
        { group: ['u-8841'], values: [12] },
        { group: ['u-2290'], values: [9] },
      ]),
      answer([{ group: ['7c2e91ab'], values: [10] }]),
    );

    expect(people).toEqual([
      { id: 'u-8841', anonymous: false, occurrences: 12 },
      { id: '7c2e91ab', anonymous: true, occurrences: 10 },
      { id: 'u-2290', anonymous: false, occurrences: 9 },
    ]);
  });
});

describe('versionSpan', () => {
  it('says where an issue was first seen and where last, or the one version it was seen in', () => {
    expect(versionSpan({ firstVersion: '2.2.0', lastVersion: '2.4.1' })).toBe('2.2.0 → 2.4.1');
    expect(versionSpan({ firstVersion: '2.4.1', lastVersion: '2.4.1' })).toBe('2.4.1');
    expect(versionSpan({ firstVersion: null, lastVersion: '2.4.1' })).toBe('2.4.1');
    expect(versionSpan({ firstVersion: null, lastVersion: null })).toBe('');
  });
});

describe('formatAgo', () => {
  const now = 1_700_000_000_000_000;
  const minutes = (count: number) => now - count * 60 * 1_000_000;

  it('says how long ago in the words of the language', () => {
    expect(formatAgo(now - 5_000_000, now, 'en')).toMatch(/now|second/);
    expect(formatAgo(minutes(3), now, 'en')).toMatch(/3 min/);
    expect(formatAgo(minutes(120), now, 'en')).toMatch(/2 hr/);
    expect(formatAgo(minutes(3 * 24 * 60), now, 'en')).toMatch(/3 days/);
    expect(formatAgo(minutes(3), now, 'zh-CN')).toContain('3');
  });
});
