import { describe, expect, it } from 'vitest';

import {
  DEFAULT_EXPLORER_PAGE_SIZE,
  EXPLORER_PAGE_SIZE_OPTIONS,
  explorerPageSizeIn,
  explorerPageSizesFor,
  MAX_EXPLORER_PAGE_SIZE,
} from './explorerPageSize';

describe('explorer page sizes', () => {
  it('start at a hundred and end at what the list endpoints give in one answer', () => {
    expect(EXPLORER_PAGE_SIZE_OPTIONS).toEqual([100, 200, 500, 1000, 2000, 5000]);
    expect(DEFAULT_EXPLORER_PAGE_SIZE).toBe(EXPLORER_PAGE_SIZE_OPTIONS[0]);
    expect(MAX_EXPLORER_PAGE_SIZE).toBe(EXPLORER_PAGE_SIZE_OPTIONS.at(-1));
  });

  it('offer nothing larger than the first size that shows every row in hand', () => {
    expect(explorerPageSizesFor(0)).toEqual([100]);
    expect(explorerPageSizesFor(80)).toEqual([100]);
    expect(explorerPageSizesFor(100)).toEqual([100]);
    expect(explorerPageSizesFor(101)).toEqual([100, 200]);
    expect(explorerPageSizesFor(200)).toEqual([100, 200]);
    expect(explorerPageSizesFor(1_200)).toEqual([100, 200, 500, 1000, 2000]);
    expect(explorerPageSizesFor(9_000)).toEqual(EXPLORER_PAGE_SIZE_OPTIONS);
  });

  it('fall back to the largest offered when the chosen size is no longer among them', () => {
    expect(explorerPageSizeIn([100, 200], 200)).toBe(200);
    expect(explorerPageSizeIn([100], 200)).toBe(100);
    expect(explorerPageSizeIn([100, 200, 500], 2000)).toBe(500);
  });
});
