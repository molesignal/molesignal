import { describe, expect, it } from 'vitest';

import type { RumAggregate, RumRow } from '@/api/rum';

import {
  ATTRIBUTIONS,
  CORE_METRICS,
  cohortFilters,
  defaultDirection,
  DIMENSIONS,
  dimensionOf,
  drawerMetricOf,
  elementsQuery,
  goodShare,
  groupFilter,
  groupLevelQuery,
  levelsQuery,
  longest,
  METRICS,
  type MetricKey,
  metricOf,
  MIN_VIEWS,
  needsImprovement,
  parseDimension,
  parseDirection,
  parseMetric,
  parseSort,
  partsQuery,
  rawOf,
  readElements,
  readGroupLevel,
  readLevels,
  readMakeup,
  readRows,
  readViews,
  readWorst,
  rescaleOf,
  type Row,
  sortRows,
  SUPPORT_METRICS,
  tableQuery,
  TABLE_ROWS,
  thresholdsOf,
  trendQuery,
  worstQuery,
  worstRowsQuery,
  worstViews,
} from './model';

const SCOPE = { application: 'storefront', environment: 'prod' };
const RANGE = { from: 10, to: 20 };

const lcp = metricOf('lcp');
const inp = metricOf('inp');
const cls = metricOf('cls');
const fcp = metricOf('fcp');

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

const ms = (milliseconds: number) => milliseconds * 1_000_000;

describe('the levels the page reads', () => {
  it('are the three vitals and the two that come before them, with the lines web.dev gives', () => {
    expect(METRICS.map((metric) => metric.key)).toEqual(['lcp', 'inp', 'cls', 'fcp', 'ttfb']);
    expect(CORE_METRICS.map((metric) => metric.key)).toEqual(['lcp', 'inp', 'cls']);
    expect(SUPPORT_METRICS.map((metric) => metric.key)).toEqual(['fcp', 'ttfb']);
    expect(METRICS.map((metric) => [metric.good, metric.poor])).toEqual([
      [2_500, 4_000],
      [200, 500],
      [0.1, 0.25],
      [1_800, 3_000],
      [800, 1_800],
    ]);
    expect(METRICS.map((metric) => metric.field)).toEqual([
      'view.performance.lcp.timestamp',
      'view.performance.inp.duration',
      'view.performance.cls.score',
      'view.performance.fcp.timestamp',
      'view.first_byte',
    ]);
  });

  it('are written to a filter in the unit the SDK stores them in: nanoseconds for a time, the score for a shift', () => {
    expect(rawOf(lcp, 2_500)).toBe('2500000000');
    expect(rawOf(inp, 200)).toBe('200000000');
    expect(rawOf(cls, 0.1)).toBe('0.1');
  });

  it('are picked by the word an address carries, and an unknown word is the first', () => {
    expect(parseMetric('fcp')).toBe('fcp');
    expect(parseMetric('nonsense')).toBe('lcp');
    expect(parseMetric(null)).toBe('lcp');
    expect(parseDimension('browser')).toBe('browser');
    expect(parseDimension('nonsense')).toBe('page');
    expect(parseSort('inp')).toBe('inp');
    expect(parseSort('nonsense')).toBe('views');
    expect(parseDirection('asc')).toBe('asc');
    expect(parseDirection('sideways')).toBeUndefined();
  });

  it('are drawn in seconds from two and a half of them and in milliseconds below, and a score is not rescaled', () => {
    expect(rescaleOf(lcp)).toEqual({ divisor: 1_000_000_000, unit: 's' });
    expect(rescaleOf(inp)).toEqual({ divisor: 1_000_000, unit: 'ms' });
    expect(rescaleOf(cls)).toBeNull();

    const labels = { good: 'Good', poor: 'Poor' };

    // The lines are told in the unit the numbers are drawn in.
    expect(thresholdsOf(lcp, labels).map((line) => line.value)).toEqual([2.5, 4]);
    expect(thresholdsOf(inp, labels).map((line) => line.value)).toEqual([200, 500]);
    expect(thresholdsOf(cls, labels).map((line) => line.value)).toEqual([0.1, 0.25]);
  });
});

describe('what the page asks of the views', () => {
  it('asks, of the vitals, the 75th percentile, how many views had each and how many were good or poor', () => {
    const query = levelsQuery(SCOPE, RANGE, CORE_METRICS);

    expect(query).toMatchObject({ type: 'views', from: 10, to: 20 });
    expect(query.filters).toEqual([
      { field: 'application.id', op: '=', value: 'storefront' },
      { field: 'env', op: '=', value: 'prod' },
    ]);
    expect(query.measures.map((measure) => measure.name)).toEqual([
      'views',
      'lcp', 'lcp_samples', 'lcp_good', 'lcp_poor',
      'inp', 'inp_samples', 'inp_good', 'inp_poor',
      'cls', 'cls_samples', 'cls_good', 'cls_poor',
    ]);
    expect(query.measures.find((measure) => measure.name === 'lcp')).toEqual({ fn: 'p75', field: 'view.performance.lcp.timestamp', name: 'lcp' });
    expect(query.measures.find((measure) => measure.name === 'lcp_samples')).toEqual({ fn: 'count', field: 'view.performance.lcp.timestamp', name: 'lcp_samples' });
    expect(query.measures.find((measure) => measure.name === 'lcp_good')?.where).toEqual([{ field: 'view.performance.lcp.timestamp', op: '<=', value: '2500000000' }]);
    expect(query.measures.find((measure) => measure.name === 'lcp_poor')?.where).toEqual([{ field: 'view.performance.lcp.timestamp', op: '>', value: '4000000000' }]);
    expect(query.measures.find((measure) => measure.name === 'cls_poor')?.where).toEqual([{ field: 'view.performance.cls.score', op: '>', value: '0.25' }]);
    // The server takes sixteen measures at the most.
    expect(query.measures.length).toBeLessThanOrEqual(16);
    expect(levelsQuery(SCOPE, RANGE, SUPPORT_METRICS).measures).toHaveLength(9);
  });

  it('leaves the environment out of what it asks when none is chosen', () => {
    expect(levelsQuery({ application: 'storefront', environment: '' }, RANGE, CORE_METRICS).filters).toEqual([
      { field: 'application.id', op: '=', value: 'storefront' },
    ]);
  });

  it('asks the table for the most viewed values, every level of each, and does not depend on the level in front', () => {
    const query = tableQuery(SCOPE, RANGE, dimensionOf('page'));

    expect(query).toMatchObject({ type: 'views', groupBy: ['view.url_path_group'], orderBy: 'views', order: 'desc', limit: TABLE_ROWS });
    // One for the views, three for each of five levels: the most the server takes.
    expect(query.measures).toHaveLength(16);
    expect(query.measures.map((measure) => measure.name)).toEqual([
      'views',
      'lcp', 'lcp_samples', 'lcp_good',
      'inp', 'inp_samples', 'inp_good',
      'cls', 'cls_samples', 'cls_good',
      'fcp', 'fcp_samples', 'fcp_good',
      'ttfb', 'ttfb_samples', 'ttfb_good',
    ]);
    expect(tableQuery(SCOPE, RANGE, dimensionOf('country')).groupBy).toEqual(['geo.country_iso_code']);
    expect(DIMENSIONS.map((dimension) => dimension.field)).toEqual([
      'view.url_path_group',
      'browser.name',
      'os.name',
      'device.type',
      'geo.country_iso_code',
      'version',
    ]);
  });

  it('asks the chart for the 75th percentile of a level in each bar, over the bars it is drawn in', () => {
    const bars = { binMicros: 5, frame: { start: 8, end: 28 } };
    const query = trendQuery(SCOPE, bars, lcp, [groupFilter(dimensionOf('page'), '/checkout')]);

    expect(query).toMatchObject({ type: 'views', from: 8, to: 28, bucketTime: 5, originTime: 8 });
    expect(query.measures).toEqual([{ fn: 'p75', field: 'view.performance.lcp.timestamp', name: 'p75' }]);
    expect(query.filters).toContainEqual({ field: 'view.url_path_group', op: '=', value: '/checkout' });
  });
});

describe('what the page reads of the levels', () => {
  const answer = answered(
    ['views', 'lcp', 'lcp_samples', 'lcp_good', 'lcp_poor', 'inp', 'inp_samples', 'inp_good', 'inp_poor', 'cls', 'cls_samples', 'cls_good', 'cls_poor'],
    [{ values: [100, ms(2_900), 80, 40, 10, ms(180), 20, 20, 0, 0.12, 100, 60, 30] }],
  );

  it('turns the 75th percentile into the unit the page reads, and grades it', () => {
    const [first, second, third] = readLevels(answer, CORE_METRICS);

    expect(first).toMatchObject({ value: 2_900, samples: 80, good: 40, poor: 10, grade: 'needs_improvement' });
    expect(second).toMatchObject({ value: 180, samples: 20, good: 20, poor: 0, grade: 'good' });
    expect(third).toMatchObject({ value: 0.12, grade: 'needs_improvement' });
    expect(readViews(answer)).toBe(100);
  });

  it('counts the views in between as what is left of those that had the level', () => {
    expect(readLevels(answer, CORE_METRICS).map(needsImprovement)).toEqual([30, 0, 10]);
  });

  it('has no value for a level no view had, and no grade for it', () => {
    const none = answered(['views', 'lcp', 'lcp_samples', 'lcp_good', 'lcp_poor'], [{ values: [5, null, 0, 0, 0] }]);

    expect(readLevels(none, [lcp])[0]).toMatchObject({ value: null, samples: 0, grade: null });
  });

  it('never counts more good or poor views than had the level', () => {
    const odd = answered(['views', 'lcp', 'lcp_samples', 'lcp_good', 'lcp_poor'], [{ values: [9, ms(1_000), 4, 9, 9] }]);

    expect(readLevels(odd, [lcp])[0]).toMatchObject({ samples: 4, good: 4, poor: 0 });
  });

  it('is empty for an answer that has not come, and for one with no row', () => {
    expect(readLevels(undefined, CORE_METRICS).map((level) => level.value)).toEqual([null, null, null]);
    expect(readViews(undefined)).toBe(0);
    expect(readViews(answered(['views'], []))).toBe(0);
  });
});

describe('the table', () => {
  const names = [
    'views',
    ...METRICS.flatMap((metric) => [metric.key, `${metric.key}_samples`, `${metric.key}_good`]),
  ];

  function row(value: string | null, views: number, levels: Partial<Record<MetricKey, number>> = {}, good: Partial<Record<MetricKey, [number, number]>> = {}) {
    return {
      group: [value],
      values: [
        views,
        ...METRICS.flatMap((metric) => [
          levels[metric.key] === undefined ? null : metric.nanoseconds ? ms(levels[metric.key]!) : levels[metric.key]!,
          good[metric.key]?.[1] ?? views,
          good[metric.key]?.[0] ?? 0,
        ]),
      ],
    };
  }

  const rows = (...entries: ReturnType<typeof row>[]) => readRows(answered(names, entries));

  it('reads each value with the level of each, in the unit the page reads it', () => {
    const [first] = rows(row('/checkout', 120, { lcp: 3_150, cls: 0.14 }, { lcp: [60, 100] }));

    expect(first).toMatchObject({ value: '/checkout', views: 120 });
    expect(first?.levels.lcp).toBe(3_150);
    expect(first?.levels.cls).toBe(0.14);
    expect(first?.levels.inp).toBeNull();
    expect(first?.samples.lcp).toBe(100);
    expect(first?.good.lcp).toBe(60);
  });

  it('says how many of the views that had a level had it good', () => {
    const [first] = rows(row('/', 50, { lcp: 1_000 }, { lcp: [30, 40] }));

    expect(goodShare(first as Row, 'lcp')).toBe(0.75);
    expect(goodShare(first as Row, 'inp')).toBe(0);
    expect(goodShare({ ...(first as Row), samples: { ...(first as Row).samples, lcp: 0 } }, 'lcp')).toBeNull();
  });

  it('is a value with none for the views that have no value of the dimension', () => {
    expect(rows(row(null, 3))[0]?.value).toBeNull();
    expect(rows(row('', 3))[0]?.value).toBeNull();
  });

  describe('in the order a reader chooses', () => {
    const table = rows(
      row('/a', 500, { lcp: 2_000 }),
      row('/b', 400, { lcp: 3_000 }),
      row('/c', 5, { lcp: 9_000 }),
      row('/d', 300, { lcp: 3_000 }),
      row('/e', 200, {}),
    );
    const names = (sorted: readonly Row[]) => sorted.map((entry) => entry.value);

    it('puts the most viewed first by the views, and the fewest first the other way', () => {
      expect(names(sortRows(table, 'views', 'desc', 'lcp'))).toEqual(['/a', '/b', '/d', '/e', '/c']);
      expect(names(sortRows(table, 'views', 'asc', 'lcp'))).toEqual(['/c', '/e', '/d', '/b', '/a']);
    });

    it('puts the slowest first by a level, a tie to the most viewed, a row with no value last', () => {
      expect(names(sortRows(table, 'lcp', 'desc', 'lcp'))).toEqual(['/b', '/d', '/a', '/e', '/c']);
    });

    it('leaves a value with too few views to the end of a ranking by a level, whichever way it runs', () => {
      expect(5).toBeLessThan(MIN_VIEWS);
      expect(names(sortRows(table, 'lcp', 'desc', 'lcp')).at(-1)).toBe('/c');
      expect(names(sortRows(table, 'lcp', 'asc', 'lcp')).at(-1)).toBe('/c');
      expect(names(sortRows(table, 'lcp', 'asc', 'lcp')).slice(0, 3)).toEqual(['/a', '/b', '/d']);
    });

    it('puts the lowest share of good views first by the share', () => {
      const shares = rows(
        row('/a', 100, { lcp: 1 }, { lcp: [90, 100] }),
        row('/b', 100, { lcp: 1 }, { lcp: [20, 100] }),
        row('/c', 100, { lcp: 1 }, { lcp: [60, 100] }),
      );

      expect(defaultDirection('good')).toBe('asc');
      expect(names(sortRows(shares, 'good', 'asc', 'lcp'))).toEqual(['/b', '/c', '/a']);
    });

    it('reads a column from the way it reads best until the reader turns it', () => {
      expect(defaultDirection('views')).toBe('desc');
      expect(defaultDirection('lcp')).toBe('desc');
    });

    it('does not change the rows it is given', () => {
      const before = names(table);

      sortRows(table, 'lcp', 'desc', 'lcp');

      expect(names(table)).toEqual(before);
    });
  });
});

describe('what a value is made of', () => {
  const group = groupFilter(dimensionOf('page'), '/checkout');

  it('asks the value its own 75th percentile first, and how many views it has', () => {
    const query = groupLevelQuery(SCOPE, RANGE, group, lcp);

    expect(query.filters).toContainEqual(group);
    expect(query.measures.map((measure) => measure.name)).toEqual(['views', 'p75', 'samples']);
    expect(readGroupLevel(answered(['views', 'p75', 'samples'], [{ values: [98, ms(3_400), 90] }]), lcp)).toEqual({
      views: 98,
      value: 3_400,
      raw: ms(3_400),
      samples: 90,
    });
    expect(readGroupLevel(answered(['views', 'p75', 'samples'], [{ values: [98, null, 0] }]), lcp)).toMatchObject({ value: null, raw: null });
    expect(readGroupLevel(answered(['views', 'p75', 'samples'], []), lcp)).toBeNull();
  });

  it('takes the views within a tenth of the 75th percentile for the views that stand for it', () => {
    expect(cohortFilters(lcp, ms(3_000))).toEqual([
      { field: 'view.performance.lcp.timestamp', op: '>=', value: '2700000000' },
      { field: 'view.performance.lcp.timestamp', op: '<=', value: '3300000000' },
    ]);
    expect(cohortFilters(cls, 0.12)).toEqual([
      { field: 'view.performance.cls.score', op: '>=', value: '0.108' },
      { field: 'view.performance.cls.score', op: '<=', value: '0.132' },
    ]);
  });

  it('asks the mean of each part over those views, and has no parts for a shift', () => {
    const query = partsQuery(SCOPE, RANGE, group, lcp, ms(3_000));

    expect(query?.measures.map((measure) => [measure.fn, measure.name])).toEqual([
      ['count', 'views'],
      ['avg', 'first_byte'],
      ['avg', 'load_delay'],
      ['avg', 'load_time'],
      ['avg', 'render_delay'],
    ]);
    expect(query?.filters).toHaveLength(2 + 3);
    expect(partsQuery(SCOPE, RANGE, group, inp, ms(300))?.measures.map((measure) => measure.name)).toEqual([
      'views',
      'input_delay',
      'processing_duration',
      'presentation_delay',
    ]);
    expect(partsQuery(SCOPE, RANGE, group, cls, 0.12)).toBeNull();
    expect(partsQuery(SCOPE, RANGE, group, fcp, ms(1_800))).toBeNull();
  });

  it('reads the parts in milliseconds, in the order they happen', () => {
    const makeup = readMakeup(
      answered(['views', 'first_byte', 'load_delay', 'load_time', 'render_delay'], [{ values: [40, ms(400), ms(500), ms(600), ms(1_900)] }]),
      lcp,
    );

    expect(makeup).toEqual({
      views: 40,
      parts: [
        { key: 'first_byte', ms: 400 },
        { key: 'load_delay', ms: 500 },
        { key: 'load_time', ms: 600 },
        { key: 'render_delay', ms: 1_900 },
      ],
    });
    expect(longest(makeup!)).toEqual({ key: 'render_delay', ms: 1_900 });
  });

  it('has no makeup when a part is missing: a bar of the rest would read as if it added up to the level', () => {
    // Safari reports the time to the first byte, and not the parts of the paint.
    expect(
      readMakeup(answered(['views', 'first_byte', 'load_delay', 'load_time', 'render_delay'], [{ values: [6, ms(228), null, null, null] }]), lcp),
    ).toBeNull();
    expect(readMakeup(answered(['views', 'first_byte', 'load_delay', 'load_time', 'render_delay'], []), lcp)).toBeNull();
    expect(readMakeup(answered(['views'], [{ values: [3] }]), cls)).toBeNull();
  });

  it('names no longest part when no part took any time', () => {
    expect(longest({ views: 1, parts: [{ key: 'a', ms: 0 }, { key: 'b', ms: 0 }] })).toBeNull();
  });

  it('asks which elements set the level, with the file where it is one, and only of the views that name one', () => {
    const query = elementsQuery(SCOPE, RANGE, group, lcp);

    expect(query?.groupBy).toEqual(['view.performance.lcp.target_selector', 'view.performance.lcp.resource_url']);
    expect(query?.filters).toContainEqual({ field: 'view.performance.lcp.target_selector', op: 'exists', value: '' });
    expect(query).toMatchObject({ orderBy: 'views', order: 'desc', limit: 6 });
    expect(elementsQuery(SCOPE, RANGE, group, inp)?.groupBy).toEqual(['view.performance.inp.target_selector']);
    expect(elementsQuery(SCOPE, RANGE, group, cls)?.groupBy).toEqual(['view.performance.cls.target_selector']);
    expect(elementsQuery(SCOPE, RANGE, group, fcp)).toBeNull();
    expect(ATTRIBUTIONS.cls?.parts).toEqual([]);
  });

  it('reads the elements with their file, their views and their 75th percentile', () => {
    const elements = readElements(
      answered(
        ['views', 'p75'],
        [
          { group: ['img.hero', 'https://cdn.example.com/hero.webp'], values: [51, ms(3_600)] },
          { group: ['h1.title', null], values: [26, ms(2_100)] },
          { group: [null, null], values: [9, ms(1_000)] },
        ],
      ),
      lcp,
    );

    expect(elements).toEqual([
      { selector: 'img.hero', resource: 'https://cdn.example.com/hero.webp', views: 51, value: 3_600 },
      { selector: 'h1.title', resource: null, views: 26, value: 2_100 },
    ]);
  });

  it('asks which views were worst, and reads them with what their rows say', () => {
    const query = worstQuery(SCOPE, RANGE, group, lcp);

    expect(query).toMatchObject({ groupBy: ['view.id'], orderBy: 'worst', order: 'desc', limit: 5 });
    expect(query.measures).toEqual([{ fn: 'max', field: 'view.performance.lcp.timestamp', name: 'worst' }]);

    const worst = readWorst(answered(['worst'], [{ group: ['v-2'], values: [ms(9_800)] }, { group: ['v-1'], values: [ms(8_700)] }]), lcp);

    expect(worst).toEqual([
      { id: 'v-2', value: 9_800 },
      { id: 'v-1', value: 8_700 },
    ]);
    expect(worstRowsQuery(SCOPE, RANGE, ['v-2', 'v-1'])).toMatchObject({ type: 'views', limit: 2 });
    expect(worstRowsQuery(SCOPE, RANGE, ['v-2', 'v-1']).filters).toContainEqual({ field: 'view.id', op: 'in', value: '["v-2","v-1"]' });

    const rows: RumRow[] = [
      { timestamp: 5, 'view.id': 'v-1', 'session.id': 's-1', 'usr.id': 'u-1', 'usr.name': 'Mei Lin', version: '2.4.0', 'browser.name': 'Chrome', 'browser.version': '126.0', 'os.name': 'macOS' },
      { timestamp: 9, 'view.id': 'v-2', 'usr.anonymous_id': 'anon-7' },
    ];

    expect(worstViews(worst, rows)).toEqual([
      { id: 'v-2', value: 9_800, at: 9, session: null, page: null, user: 'anon-7', version: null, browser: null, system: null },
      { id: 'v-1', value: 8_700, at: 5, session: 's-1', page: null, user: 'Mei Lin', version: '2.4.0', browser: 'Chrome 126.0', system: 'macOS' },
    ]);
    // A view whose row is not there (yet) is told by its id alone.
    expect(worstViews(worst, [])[0]).toMatchObject({ id: 'v-2', at: null, user: null });
  });

  it('opens on the vital in front, or the first of them when the one in front is read beside them', () => {
    expect(drawerMetricOf(null, 'inp')).toBe('inp');
    expect(drawerMetricOf(null, 'fcp')).toBe('lcp');
    expect(drawerMetricOf('cls', 'lcp')).toBe('cls');
    expect(drawerMetricOf('fcp', 'inp')).toBe('inp');
  });
});
