import { describe, expect, it } from 'vitest';

import type { RumAggregate } from '@/api/rum';

import { keptShare, keptShareQuery } from './EstimateNote';

const answer = (values: Array<number | string | null>): RumAggregate => ({
  groupBy: [],
  measures: [],
  rows: [{ group: [], values }],
});

describe('keptShare', () => {
  it('is the sessions that are stored against the sessions they stand for', () => {
    expect(keptShare(answer([4000, 1000]))).toBe(0.25);
    expect(keptShare(answer([1000, 1000]))).toBe(1);
  });

  it('is no share without sessions or without numbers', () => {
    expect(keptShare(answer([0, 0]))).toBeNull();
    expect(keptShare(answer([null, null]))).toBeNull();
    expect(keptShare(answer(['masked', 3]))).toBeNull();
    expect(keptShare(undefined)).toBeNull();
    expect(keptShare({ groupBy: [], measures: [], rows: [] })).toBeNull();
  });

  it('is never more than all of them', () => {
    expect(keptShare(answer([90, 100]))).toBe(1);
  });
});

describe('keptShareQuery', () => {
  it('asks the sessions of the application and its environment, as many as they stand for and as stored', () => {
    const query = keptShareQuery('shop', 'prod', { from: 10, to: 20 });

    expect(query.type).toBe('sessions');
    expect(query.filters).toEqual([
      { field: 'application.id', op: '=', value: 'shop' },
      { field: 'env', op: '=', value: 'prod' },
    ]);
    expect(query.measures.map((measure) => `${measure.fn}:${measure.name}`)).toEqual(['count:estimated', 'stored:stored']);
    expect(query.estimate).toBe(true);
  });
});
