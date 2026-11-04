import { describe, expect, it } from 'vitest';

import type { RumField, RumFieldType, RumUnit } from '@/api/rum';

import { readRumVizAddress } from './address';
import {
  autoBucketOf,
  barsFor,
  bucketChoices,
  COUNT,
  COUNTRY_FIELD,
  defaultGroup,
  groupFields,
  measureFields,
  measureName,
  measureOfToken,
  measureToken,
  resolveVizSpec,
  vizRequest,
} from './spec';

const field = (name: string, data_type: RumFieldType, unit: RumUnit = 'none'): RumField => ({
  name,
  dataType: data_type,
  unit,
  facetable: data_type !== 'json',
  indexed: false,
});

const FIELDS: RumField[] = [
  field('browser.name', 'utf8'),
  field('view.url_path_group', 'utf8'),
  field(COUNTRY_FIELD, 'utf8'),
  field('view.performance.lcp.timestamp', 'int64', 'ns'),
  field('view.error.count', 'int64', 'count'),
  field('view.performance.cls.score', 'float64', 'score'),
  field('session.id', 'utf8'),
  field('context', 'json'),
  field('date', 'timestamp'),
];

const HOUR = 3_600_000_000;
const RANGE = { from: 1_760_000_000_000_000, to: 1_760_000_000_000_000 + 24 * HOUR };
const asked = (query: string) => readRumVizAddress(new URLSearchParams(query));

describe('the fields a measure and a group can take', () => {
  it('counts rows without a field, any field with values for a distinct count, and numbers for the rest', () => {
    expect(measureFields('count', FIELDS)).toEqual([]);
    expect(measureFields('count_distinct', FIELDS).map((entry) => entry.name)).toContain('session.id');
    expect(measureFields('count_distinct', FIELDS).map((entry) => entry.name)).not.toContain('context');
    expect(measureFields('p75', FIELDS).map((entry) => entry.name)).toEqual([
      'view.performance.lcp.timestamp',
      'view.error.count',
      'view.performance.cls.score',
    ]);
  });

  it('groups by a value that repeats: not a measurement, a time or a document', () => {
    expect(groupFields(FIELDS).map((entry) => entry.name)).toEqual([
      'browser.name',
      'view.url_path_group',
      COUNTRY_FIELD,
      'view.error.count',
      'session.id',
    ]);
  });

  it('opens a ranking on what the type is most often ranked by, else on a common field', () => {
    expect(defaultGroup('views', FIELDS)).toBe('view.url_path_group');
    expect(defaultGroup('sessions', FIELDS)).toBe('browser.name');
    expect(defaultGroup('errors', FIELDS)).toBe('browser.name');
    expect(defaultGroup('views', [field('context', 'json')])).toBeNull();
  });
});

describe('a measure in the address', () => {
  it('is a function, and the field it takes unless it counts rows', () => {
    expect(measureOfToken('count', FIELDS)).toEqual(COUNT);
    expect(measureOfToken('p75:view.performance.lcp.timestamp', FIELDS)).toEqual({ fn: 'p75', field: 'view.performance.lcp.timestamp' });
    expect(measureToken({ fn: 'p75', field: 'view.performance.lcp.timestamp' })).toBe('p75:view.performance.lcp.timestamp');
    expect(measureName({ fn: 'count_distinct', field: 'session.id' })).toBe('count_distinct(session.id)');
    expect(measureName(COUNT)).toBe('count');
  });

  it('is dropped when the type cannot answer it', () => {
    expect(measureOfToken('p75:browser.name', FIELDS)).toBeNull();
    expect(measureOfToken('p75', FIELDS)).toBeNull();
    expect(measureOfToken('count:view.error.count', FIELDS)).toBeNull();
    expect(measureOfToken('p75:missing.field', FIELDS)).toBeNull();
    expect(measureOfToken('median:view.error.count', FIELDS)).toBeNull();
    expect(measureOfToken('count_distinct:context', FIELDS)).toBeNull();
  });
});

describe('what an address asks to be drawn', () => {
  it('is a count, by nothing, for a chart that is asked nothing', () => {
    expect(resolveVizSpec('timeseries', asked(''), 'views', FIELDS)).toEqual({
      viz: 'timeseries',
      measures: [COUNT],
      groups: [],
      bucket: null,
      top: 5,
      sort: null,
      field: null,
    });
  });

  it('ranks by the type\'s own field when a ranking or a table is asked nothing', () => {
    expect(resolveVizSpec('toplist', asked(''), 'views', FIELDS).groups).toEqual(['view.url_path_group']);
    expect(resolveVizSpec('table', asked(''), 'views', FIELDS).groups).toEqual(['view.url_path_group']);
    expect(resolveVizSpec('toplist', asked(''), 'views', FIELDS).top).toBe(10);
  });

  it('takes the groups and measures it names, as many as the way of drawing carries', () => {
    const query = 'm=p75:view.performance.lcp.timestamp&m=count&by=browser.name&by=session.id&by=view.url_path_group';

    const chart = resolveVizSpec('timeseries', asked(query), 'views', FIELDS);
    expect(chart.measures).toEqual([{ fn: 'p75', field: 'view.performance.lcp.timestamp' }]);
    expect(chart.groups).toEqual(['browser.name', 'session.id']);

    const table = resolveVizSpec('table', asked(query), 'views', FIELDS);
    expect(table.measures).toHaveLength(2);
    expect(table.groups).toEqual(['browser.name', 'session.id']);

    expect(resolveVizSpec('toplist', asked(query), 'views', FIELDS).groups).toEqual(['browser.name']);
  });

  it('leaves out what the type does not have, and what is named twice', () => {
    const spec = resolveVizSpec('table', asked('m=sum:nope&m=count&m=count&by=nope&by=browser.name&by=browser.name'), 'views', FIELDS);

    expect(spec.measures).toEqual([COUNT]);
    expect(spec.groups).toEqual(['browser.name']);
  });

  it('tells a distribution by one number, the one asked for or the first the type has', () => {
    const numbers = [
      { name: 'view.loading_time', dataType: 'int64', unit: 'ns', facetable: false, indexed: false },
      { name: 'view.time_spent', dataType: 'int64', unit: 'ns', facetable: false, indexed: false },
      { name: 'browser.name', dataType: 'utf8', unit: 'none', facetable: true, indexed: false },
    ] as const;

    expect(resolveVizSpec('distribution', asked('field=view.time_spent'), 'views', numbers)).toMatchObject({
      field: 'view.time_spent',
      measures: [],
      groups: [],
    });
    expect(resolveVizSpec('distribution', asked('field=browser.name'), 'views', numbers).field).toBe('view.loading_time');
    expect(resolveVizSpec('distribution', asked(''), 'sessions', numbers).field).toBe('view.loading_time');
    expect(resolveVizSpec('distribution', asked(''), 'views', [numbers[2]]).field).toBeNull();
    expect(resolveVizSpec('table', asked('field=view.time_spent'), 'views', numbers).field).toBeNull();
  });

  it('groups a map by country, whatever the address says', () => {
    expect(resolveVizSpec('map', asked('by=browser.name&top=5'), 'sessions', FIELDS).groups).toEqual([COUNTRY_FIELD]);
  });

  it('keeps a number of groups that is one of the choices, else the nearest below', () => {
    expect(resolveVizSpec('toplist', asked('top=20'), 'views', FIELDS).top).toBe(20);
    expect(resolveVizSpec('toplist', asked('top=30'), 'views', FIELDS).top).toBe(20);
    expect(resolveVizSpec('toplist', asked('top=1'), 'views', FIELDS).top).toBe(5);
    expect(resolveVizSpec('timeseries', asked('top=100'), 'views', FIELDS).top).toBe(20);
    expect(resolveVizSpec('timeseries', asked('top=banana'), 'views', FIELDS).top).toBe(5);
  });

  it('orders a table only by one of its own columns', () => {
    expect(resolveVizSpec('table', asked('sort=count&dir=asc'), 'views', FIELDS).sort).toEqual({ by: 'count', descending: false });
    expect(resolveVizSpec('table', asked('sort=view.url_path_group'), 'views', FIELDS).sort).toEqual({ by: 'view.url_path_group', descending: false });
    expect(resolveVizSpec('table', asked('sort=count'), 'views', FIELDS).sort).toEqual({ by: 'count', descending: true });
    expect(resolveVizSpec('table', asked('sort=p75(view.error.count)'), 'views', FIELDS).sort).toBeNull();
    expect(resolveVizSpec('toplist', asked('sort=count'), 'views', FIELDS).sort).toBeNull();
  });

  it('keeps a bucket of time the address names, and none for a word it does not know', () => {
    expect(resolveVizSpec('timeseries', asked('bucket=1h'), 'views', FIELDS).bucket).toBe(HOUR);
    expect(resolveVizSpec('timeseries', asked('bucket=90m'), 'views', FIELDS).bucket).toBeNull();
  });
});

describe('the bars of a chart over time', () => {
  it('are about thirty to a range, each a length a reader counts in', () => {
    expect(autoBucketOf({ start: 0, end: 24 * HOUR })).toBe(HOUR);
    expect(autoBucketOf({ start: 0, end: HOUR })).toBe(2 * 60_000_000);
    expect(autoBucketOf({ start: 0, end: 7 * 24 * HOUR })).toBe(6 * HOUR);
  });

  it('begin on the reader\'s hours, and cover the range', () => {
    // A quarter past five in a zone eight hours ahead of UTC.
    const start = 9 * HOUR + 15 * 60_000_000;
    const bars = barsFor({ start, end: start + 3 * HOUR }, HOUR, 8 * HOUR);

    expect(bars.binMicros).toBe(HOUR);
    expect((bars.frame.start + 8 * HOUR) % HOUR).toBe(0);
    expect(bars.frame.start).toBeLessThanOrEqual(start);
    expect(bars.frame.end).toBeGreaterThanOrEqual(start + 3 * HOUR);
  });

  it('are fewer than a chart carries: a length that would cut the range finer is replaced', () => {
    const bars = barsFor({ start: 0, end: 7 * 24 * HOUR }, 10_000_000, 0);

    expect((bars.frame.end - bars.frame.start) / bars.binMicros).toBeLessThanOrEqual(300);
    expect(bucketChoices({ start: 0, end: 24 * HOUR })).toEqual([5, 15, 30, 60, 180, 360, 720].map((minutes) => minutes * 60_000_000));
  });
});

describe('the read that answers a spec', () => {
  const context = { tab: 'views' as const, range: RANGE, filters: [], freeText: [], utcOffsetMicros: 0 };

  it('asks a chart for bars of time over the frame, from where the first begins', () => {
    const spec = resolveVizSpec('timeseries', asked('m=p75:view.performance.lcp.timestamp&by=view.url_path_group'), 'views', FIELDS);
    const request = vizRequest(spec, context);

    expect(request.bars).not.toBeNull();
    expect(request.params).toMatchObject({
      type: 'views',
      measures: [{ fn: 'p75', field: 'view.performance.lcp.timestamp' }],
      groupBy: ['view.url_path_group'],
      bucketTime: request.bars!.binMicros,
      originTime: request.bars!.frame.start,
      from: request.bars!.frame.start,
      to: request.bars!.frame.end,
      limit: 5,
    });
  });

  it('asks a ranking for the range it was given, with no bars', () => {
    const request = vizRequest(resolveVizSpec('toplist', asked(''), 'views', FIELDS), {
      ...context,
      filters: [{ field: 'browser.name', op: '=', value: 'Chrome' }],
      freeText: ['checkout'],
    });

    expect(request.bars).toBeNull();
    expect(request.params).toEqual({
      type: 'views',
      from: RANGE.from,
      to: RANGE.to,
      filters: [{ field: 'browser.name', op: '=', value: 'Chrome' }],
      freeText: ['checkout'],
      measures: [{ fn: 'count' }],
      groupBy: ['view.url_path_group'],
      limit: 10,
      estimate: false,
    });
  });

  it('asks a table for its order by the name the server gives the column', () => {
    const spec = resolveVizSpec('table', asked('m=count&m=avg:view.error.count&sort=avg(view.error.count)&dir=asc'), 'views', FIELDS);

    expect(vizRequest(spec, context).params).toMatchObject({ orderBy: 'avg(view.error.count)', order: 'asc' });
  });

  it('leaves a length that is not one of the range\'s choices to the range', () => {
    const spec = resolveVizSpec('timeseries', asked('bucket=10s'), 'views', FIELDS);
    const request = vizRequest(spec, context);

    expect(request.bars!.binMicros).toBe(HOUR);
  });
});
