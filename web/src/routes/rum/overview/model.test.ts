import { describe, expect, it } from 'vitest';

import type { RumAggregate } from '@/api/rum';

import {
  countriesQuery,
  crashFreeSessions,
  crashFreeUsers,
  errorFreeSessions,
  formatVital,
  frustrationQuery,
  gradeOf,
  inUnit,
  markerOf,
  pagesQuery,
  performancePath,
  pulseQuery,
  readCountries,
  readFrustration,
  readPages,
  readPulse,
  readShares,
  readVitals,
  scaleOf,
  scopeFilters,
  sharesQuery,
  VITALS,
  vitalsQuery,
  withOthers,
} from './model';

const SCOPE = { application: 'storefront', environment: 'prod' };
const RANGE = { from: 10, to: 20 };

function answered(
  measures: string[],
  rows: Array<{ group?: Array<string | null>; values: Array<number | string | null> }>,
): RumAggregate {
  return {
    groupBy: [],
    measures: measures.map((name) => ({ name, fn: 'count', unit: 'count' })),
    rows: rows.map((row) => ({ group: row.group ?? [], values: row.values })),
  };
}

const lcp = VITALS[0]!;
const inp = VITALS[1]!;
const cls = VITALS[2]!;

describe('what every read of the overview carries', () => {
  it('is the application, and the environment when one is chosen', () => {
    expect(scopeFilters({ application: 'storefront', environment: '' })).toEqual([
      { field: 'application.id', op: '=', value: 'storefront' },
    ]);
    expect(scopeFilters(SCOPE, [{ field: 'x', op: '>', value: '0' }])).toEqual([
      { field: 'application.id', op: '=', value: 'storefront' },
      { field: 'env', op: '=', value: 'prod' },
      { field: 'x', op: '>', value: '0' },
    ]);
  });

  it('asks the sessions how many there were, of how many people, and how many met an error or a crash', () => {
    const query = pulseQuery(SCOPE, RANGE);

    expect(query).toMatchObject({ type: 'sessions', from: 10, to: 20 });
    expect(query.measures.map((measure) => measure.name)).toEqual([
      'sessions',
      'users',
      'with_errors',
      'with_crashes',
      'users_with_crashes',
    ]);
    // A person without an id is told by the anonymous one.
    expect(query.measures[1]).toMatchObject({ fn: 'count_distinct', field: 'usr.id', fallback: 'usr.anonymous_id' });
    expect(query.measures[2]).toMatchObject({ fn: 'count', where: [{ field: 'session.error.count', op: '>', value: '0' }] });
  });

  it('asks the views for the 75th percentile of each vital and how many views had one', () => {
    const query = vitalsQuery(SCOPE, RANGE);

    expect(query.type).toBe('views');
    expect(query.measures.map((measure) => `${measure.fn}:${measure.name}`)).toEqual([
      'p75:lcp',
      'count:lcp_samples',
      'p75:inp',
      'count:inp_samples',
      'p75:cls',
      'count:cls_samples',
    ]);
  });

  it('tells the pages of a browser by their path and the screens of an app by their name', () => {
    const browser = pagesQuery(SCOPE, RANGE, 'browser');
    const app = pagesQuery(SCOPE, RANGE, 'flutter');

    expect(browser.groupBy).toEqual(['view.url_path_group']);
    expect(browser.measures.map((measure) => measure.name)).toEqual(['views', 'lcp', 'inp', 'cls', 'with_errors']);
    expect(app.groupBy).toEqual(['view.name']);
    // An app sends no web vitals to ask for.
    expect(app.measures.map((measure) => measure.name)).toEqual(['views', 'with_errors']);
    expect(browser).toMatchObject({ orderBy: 'views', limit: 8 });
  });

  it('asks for every country, the most used column values, and the elements actions were frustrated on', () => {
    expect(countriesQuery(SCOPE, RANGE)).toMatchObject({ groupBy: ['geo.country_iso_code'], limit: 250 });
    expect(sharesQuery(SCOPE, RANGE, 'browser.name')).toMatchObject({ groupBy: ['browser.name'], limit: 6 });

    const frustrated = frustrationQuery(SCOPE, RANGE, 'browser');

    expect(frustrated.type).toBe('actions');
    expect(frustrated.filters).toContainEqual({ field: 'action.frustration.count', op: '>', value: '0' });
    expect(frustrated.groupBy).toEqual(['action.target.name', 'view.url_path_group']);
    expect(frustrated.measures.map((measure) => measure.name)).toEqual(['total', 'rage', 'dead', 'error']);
    expect(frustrated.measures[1]).toMatchObject({ where: [{ field: 'action.frustration.type', op: 'contains', value: 'rage_' }] });
  });
});

describe('the vitals', () => {
  it('are graded with the thresholds web.dev gives, the thresholds themselves being good and fair', () => {
    expect(gradeOf(lcp, 2_500)).toBe('good');
    expect(gradeOf(lcp, 2_500.1)).toBe('needs_improvement');
    expect(gradeOf(lcp, 4_000)).toBe('needs_improvement');
    expect(gradeOf(lcp, 4_001)).toBe('poor');
    expect(gradeOf(inp, 200)).toBe('good');
    expect(gradeOf(cls, 0.25)).toBe('needs_improvement');
    expect(gradeOf(cls, 0.31)).toBe('poor');
  });

  it('are read in milliseconds, and a score as it is', () => {
    expect(inUnit(lcp, 2_100_000_000)).toBe(2_100);
    expect(inUnit(cls, 0.06)).toBe(0.06);
  });

  it('stand on their scale, a value past its end at the end', () => {
    expect(markerOf(lcp, 2_100)).toBeCloseTo(0.35);
    expect(markerOf(inp, 240)).toBeCloseTo(0.3);
    expect(markerOf(cls, 0.06)).toBeCloseTo(0.15);
    expect(markerOf(lcp, 99_000)).toBe(1);
    expect(markerOf(lcp, -1)).toBe(0);

    for (const vital of VITALS) {
      expect(scaleOf(vital).reduce((sum, part) => sum + part, 0)).toBeCloseTo(1);
    }
  });

  it('are written as time or as a score', () => {
    expect(formatVital(lcp, 2_140)).toBe('2.1 s');
    expect(formatVital(inp, 240)).toBe('240 ms');
    expect(formatVital(cls, 0.0612)).toBe('0.06');
    expect(formatVital(cls, 0)).toBe('0');
  });

  it('are read with the number of views that had them; none without', () => {
    const readings = readVitals(
      answered(
        ['lcp', 'lcp_samples', 'inp', 'inp_samples', 'cls', 'cls_samples'],
        [{ values: [2_100_000_000, 40, 240_000_000, 0, '***', 12] }],
      ),
    );

    expect(readings.map((reading) => [reading.vital.key, reading.value, reading.samples, reading.grade])).toEqual([
      ['lcp', 2_100, 40, 'good'],
      // A percentile of no views is no value, whatever the engine puts there.
      ['inp', null, 0, null],
      // A value the reader may not see is no value either.
      ['cls', null, 12, null],
    ]);
    expect(readVitals(undefined).every((reading) => reading.value === null)).toBe(true);
  });
});

describe('what the sessions came to', () => {
  const pulse = readPulse(
    answered(
      ['sessions', 'users', 'with_errors', 'with_crashes', 'users_with_crashes'],
      [{ values: [1_000, 400, 18, 5, 4] }],
    ),
  );

  it('are the share without an error, and without a crash for sessions and for people', () => {
    expect(pulse).toEqual({ sessions: 1_000, users: 400, withErrors: 18, withCrashes: 5, usersWithCrashes: 4 });
    expect(errorFreeSessions(pulse!)).toBeCloseTo(0.982);
    expect(crashFreeSessions(pulse!)).toBeCloseTo(0.995);
    expect(crashFreeUsers(pulse!)).toBeCloseTo(0.99);
  });

  it('have no share when there were none, and no pulse before there is an answer or when what it says is masked', () => {
    const none = readPulse(answered(['sessions', 'users', 'with_errors'], [{ values: [0, 0, 0] }]))!;

    expect(errorFreeSessions(none)).toBeNull();
    expect(crashFreeUsers(none)).toBeNull();
    // An answer without a row is no sessions, not a missing answer.
    expect(readPulse(answered(['sessions'], []))).toEqual({
      sessions: 0,
      users: 0,
      withErrors: 0,
      withCrashes: 0,
      usersWithCrashes: 0,
    });
    expect(readPulse(answered(['sessions'], [{ values: ['***'] }]))).toBeNull();
    expect(readPulse(undefined)).toBeNull();
  });
});

describe('the pages', () => {
  it('are read with their vitals in milliseconds and the share of their views that met an error', () => {
    const pages = readPages(
      answered(
        ['views', 'lcp', 'inp', 'cls', 'with_errors'],
        [
          { group: ['/checkout'], values: [800, 3_400_000_000, 420_000_000, 0.19, 30] },
          { group: ['/'], values: [4_800, 1_900_000_000, null, 0.03, 0] },
          // A view without a page has none to be told by, and what is masked is not a number.
          { group: [null], values: [20, 1, 1, 1, 1] },
          { group: ['/cart'], values: [100, '***', 90_000_000, 0.05, 2] },
        ],
      ),
    );

    expect(pages).toEqual([
      { page: '/checkout', views: 800, lcp: 3_400, inp: 420, cls: 0.19, errorRate: 30 / 800 },
      { page: '/', views: 4_800, lcp: 1_900, inp: null, cls: 0.03, errorRate: 0 },
      { page: '/cart', views: 100, lcp: null, inp: 90, cls: 0.05, errorRate: 0.02 },
    ]);
  });

  it('of an app have no vitals', () => {
    const [screen] = readPages(answered(['views', 'with_errors'], [{ group: ['Cart'], values: [10, 1] }]));

    expect(screen).toEqual({ page: 'Cart', views: 10, lcp: null, inp: null, cls: null, errorRate: 0.1 });
  });
});

describe('the spread of sessions', () => {
  const browsers = readShares(
    answered(
      ['sessions'],
      [
        { group: ['Chrome'], values: [580] },
        { group: ['Safari'], values: [220] },
        { group: [null], values: [120] },
        { group: ['Edge'], values: [80] },
      ],
    ),
  );

  it('are the values that have sessions, the commonest first', () => {
    expect(browsers).toEqual([
      { value: 'Chrome', sessions: 580 },
      { value: 'Safari', sessions: 220 },
      { value: 'Edge', sessions: 80 },
    ]);
  });

  it('leave the rest of the total to the others, the sessions with no value among them', () => {
    expect(withOthers(browsers, 1_000, 2)).toEqual({
      shown: [
        { value: 'Chrome', sessions: 580 },
        { value: 'Safari', sessions: 220 },
      ],
      others: 200,
    });
    expect(withOthers(browsers, 880, 5).others).toBe(0);
  });

  it('of countries are the codes and their sessions', () => {
    expect(readCountries(answered(['sessions'], [{ group: ['US'], values: [38] }, { group: [null], values: [2] }]))).toEqual([
      { code: 'US', sessions: 38 },
    ]);
  });
});

describe('the frustrating elements', () => {
  it('are named by what most of their actions were, a tie going to the rage', () => {
    const found = readFrustration(
      answered(
        ['total', 'rage', 'dead', 'error'],
        [
          { group: ['Pay now', '/checkout'], values: [214, 150, 40, 24] },
          { group: ['Banner', '/'], values: [168, 0, 168, 0] },
          { group: ['Add to cart', '/products/:sku'], values: [64, 0, 0, 64] },
          { group: [null, '/cart'], values: [10, 5, 5, 0] },
          { group: ['Empty', '/x'], values: [0, 0, 0, 0] },
        ],
      ),
    );

    expect(found).toEqual([
      { element: 'Pay now', page: '/checkout', total: 214, kind: 'rage' },
      { element: 'Banner', page: '/', total: 168, kind: 'dead' },
      { element: 'Add to cart', page: '/products/:sku', total: 64, kind: 'error' },
      { element: null, page: '/cart', total: 10, kind: 'rage' },
    ]);
  });
});

describe('where a vital of the overview leads', () => {
  it('is the performance page of the same application and environment, with that vital in front', () => {
    expect(performancePath(SCOPE, 'inp')).toBe('/rum/performance?app=storefront&env=prod&vital=inp');
    expect(performancePath(SCOPE, 'cls')).toBe('/rum/performance?app=storefront&env=prod&vital=cls');
  });

  it('leaves out what the page opens on: the first vital, and no environment', () => {
    expect(performancePath(SCOPE, 'lcp')).toBe('/rum/performance?app=storefront&env=prod');
    expect(performancePath({ application: 'shop web', environment: '' }, 'lcp')).toBe('/rum/performance?app=shop+web');
  });
});
