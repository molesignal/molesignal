import { describe, expect, it } from 'vitest';

import {
  calculateHomeStreamRowCount,
  DEFAULT_HOME_STREAM_ROWS,
  shouldFillHomeStreamViewport,
} from './streamRows';

describe('home stream row count', () => {
  it('fills the available table height with complete rows', () => {
    expect(
      calculateHomeStreamRowCount({
        viewportHeight: 500,
        headerHeight: 44,
        rowHeight: 44,
        totalRows: 20,
      }),
    ).toBe(10);
  });

  it('adapts to compact density row heights', () => {
    expect(
      calculateHomeStreamRowCount({
        viewportHeight: 500,
        headerHeight: 34,
        rowHeight: 34,
        totalRows: 20,
      }),
    ).toBe(13);
  });

  it('never exceeds the available stream count', () => {
    expect(
      calculateHomeStreamRowCount({
        viewportHeight: 800,
        headerHeight: 44,
        rowHeight: 44,
        totalRows: 6,
      }),
    ).toBe(6);
  });

  it('keeps a stable fallback before the browser has measured the table', () => {
    expect(
      calculateHomeStreamRowCount({
        viewportHeight: 0,
        headerHeight: 0,
        rowHeight: 0,
        totalRows: 20,
      }),
    ).toBe(DEFAULT_HOME_STREAM_ROWS);
  });

  it('shows one row in a constrained measured viewport and none without data', () => {
    expect(
      calculateHomeStreamRowCount({
        viewportHeight: 50,
        headerHeight: 44,
        rowHeight: 44,
        totalRows: 20,
      }),
    ).toBe(1);
    expect(
      calculateHomeStreamRowCount({
        viewportHeight: 500,
        headerHeight: 44,
        rowHeight: 44,
        totalRows: 0,
      }),
    ).toBe(0);
  });

  it('fills only a partial-row remainder', () => {
    expect(
      shouldFillHomeStreamViewport({
        viewportHeight: 477,
        headerHeight: 34,
        rowHeight: 39,
        visibleRows: 11,
      }),
    ).toBe(true);
    expect(
      shouldFillHomeStreamViewport({
        viewportHeight: 477,
        headerHeight: 34,
        rowHeight: 39,
        visibleRows: 8,
      }),
    ).toBe(false);
  });

  it('does not stretch an overflowing or unmeasured table', () => {
    expect(
      shouldFillHomeStreamViewport({
        viewportHeight: 440,
        headerHeight: 44,
        rowHeight: 44,
        visibleRows: 10,
      }),
    ).toBe(false);
    expect(
      shouldFillHomeStreamViewport({
        viewportHeight: 0,
        headerHeight: 0,
        rowHeight: 0,
        visibleRows: 8,
      }),
    ).toBe(false);
  });
});
