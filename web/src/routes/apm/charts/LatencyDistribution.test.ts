import { describe, expect, it } from 'vitest';

import { rangeLabel, rowsOf } from './LatencyDistribution';

describe('latency distribution rows', () => {
  it('puts each bucket in the 1-2-5 range its middle falls in, keeping the empty ranges between', () => {
    const rows = rowsOf([
      { lowerTime: 1_000, upperTime: 1_090, count: 2 },
      { lowerTime: 1_090, upperTime: 1_189, count: 3 },
      { lowerTime: 64_000, upperTime: 69_800, count: 1 },
    ]);

    expect(rows.map((row) => [rangeLabel(row.lower, row.upper), row.count])).toEqual([
      ['1–2 ms', 5],
      ['2–5 ms', 0],
      ['5–10 ms', 0],
      ['10–20 ms', 0],
      ['20–50 ms', 0],
      ['50–100 ms', 1],
    ]);
    expect(rows.reduce((sum, row) => sum + row.count, 0)).toBe(6);
    expect(rowsOf([])).toEqual([]);
  });

  it('says a range with its unit once when both ends share it', () => {
    expect(rangeLabel(500, 1_000)).toBe('500 µs–1 ms');
    expect(rangeLabel(500_000, 1_000_000)).toBe('500 ms–1 s');
    expect(rangeLabel(2_000_000, 5_000_000)).toBe('2–5 s');
  });
});
