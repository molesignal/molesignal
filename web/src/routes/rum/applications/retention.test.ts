import { describe, expect, it } from 'vitest';

import type { RetentionFilter } from '@/api/rum';

import {
  distinctNames,
  MAX_RETENTION_CONDITIONS,
  MAX_RETENTION_FILTERS,
  MAX_RETENTION_VALUES,
  newFilter,
  parseValues,
  sameFilters,
  sameValues,
  validFilter,
  validFilters,
} from './retention';

const filter = (over: Partial<RetentionFilter> = {}): RetentionFilter => ({
  id: '',
  name: 'Production',
  rate: 100,
  conditions: [{ field: 'env', op: 'is', values: ['prod'] }],
  ...over,
});

describe('parseValues', () => {
  it('lists the words between commas, trimmed, each once whatever its case', () => {
    expect(parseValues('prod, Production , PROD,staging')).toEqual(['prod', 'Production', 'staging']);
    expect(parseValues('')).toEqual([]);
    expect(parseValues(' , ,')).toEqual([]);
  });

  it('takes Chinese commas, semicolons and lines as commas', () => {
    expect(parseValues('生产，测试、预发；beta\ngamma')).toEqual(['生产', '测试', '预发', 'beta', 'gamma']);
  });

  it('says whether two lists are the same, in order', () => {
    expect(sameValues(['a', 'b'], ['a', 'b'])).toBe(true);
    expect(sameValues(['a', 'b'], ['b', 'a'])).toBe(false);
    expect(sameValues(['a'], ['a', 'b'])).toBe(false);
  });
});

describe('newFilter', () => {
  const named = (number: number) => `Filter ${number}`;

  it('keeps every session it describes, with a condition on the environment to fill in', () => {
    const added = newFilter([], named);

    expect(added).toEqual({
      id: '',
      name: 'Filter 1',
      rate: 100,
      conditions: [{ field: 'env', op: 'is', values: [] }],
    });
    expect(validFilter(added)).toBe(false);
  });

  it('is named after the filters there are, and apart from them', () => {
    expect(newFilter(['Production'], named).name).toBe('Filter 2');
    expect(newFilter(['Production', 'filter 2'], named).name).toBe('Filter 3');
    expect(newFilter(['Filter 1'], named).name).toBe('Filter 2');
  });
});

describe('validFilters', () => {
  it('takes a filter with a name, a whole share and values in every condition', () => {
    expect(validFilter(filter())).toBe(true);
    expect(validFilter(filter({ conditions: [] }))).toBe(true);
    expect(validFilter(filter({ name: ' ' }))).toBe(false);
    expect(validFilter(filter({ name: 'n'.repeat(65) }))).toBe(false);
    expect(validFilter(filter({ rate: 100.5 }))).toBe(false);
    expect(validFilter(filter({ rate: -1 }))).toBe(false);
    expect(validFilter(filter({ conditions: [{ field: 'env', op: 'is', values: [] }] }))).toBe(false);
  });

  it('has limits to the conditions, the values of one, and the filters', () => {
    const conditions = Array.from({ length: MAX_RETENTION_CONDITIONS + 1 }, () => ({
      field: 'env' as const,
      op: 'is' as const,
      values: ['x'],
    }));
    const values = Array.from({ length: MAX_RETENTION_VALUES + 1 }, (_, index) => String(index));

    expect(validFilter(filter({ conditions }))).toBe(false);
    expect(validFilter(filter({ conditions: [{ field: 'version', op: 'is', values }] }))).toBe(false);
    expect(validFilter(filter({ conditions: [{ field: 'version', op: 'is', values: ['v'.repeat(129)] }] }))).toBe(false);

    const many = Array.from({ length: MAX_RETENTION_FILTERS + 1 }, (_, index) => filter({ name: `F${index}` }));

    expect(validFilters(many.slice(0, MAX_RETENTION_FILTERS))).toBe(true);
    expect(validFilters(many)).toBe(false);
  });

  it('tell filters apart by their names, whatever the case', () => {
    expect(distinctNames([filter({ name: 'A' }), filter({ name: 'B' })])).toBe(true);
    expect(distinctNames([filter({ name: 'A' }), filter({ name: ' a ' })])).toBe(false);
    expect(validFilters([filter({ name: 'A' }), filter({ name: 'a' })])).toBe(false);
  });
});

describe('sameFilters', () => {
  it('is true for lists that say the same in the same order', () => {
    expect(sameFilters([filter()], [filter()])).toBe(true);
    expect(sameFilters([], [])).toBe(true);
    expect(sameFilters([filter()], [])).toBe(false);
    expect(sameFilters([filter({ id: 'a' })], [filter({ id: 'b' })])).toBe(false);
    expect(sameFilters([filter()], [filter({ conditions: [{ field: 'env', op: 'is_not', values: ['prod'] }] })])).toBe(false);
  });
});
