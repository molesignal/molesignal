import { describe, expect, it } from 'vitest';

import type { DiffFlamebearer, Flamebearer } from '@/api/profiles';

import {
  ancestorAtDepth,
  callTreeChildren,
  decodeNodes,
  diffFunctions,
  diffIntensity,
  diffTone,
  diffTotals,
  formatBytes,
  formatChange,
  formatChangePct,
  formatDuration,
  frameLabel,
  heatColor,
  isRootFrame,
  nodeInWindow,
  nodeKey,
  rootTotal,
  topFunctions,
  unitForProfileType,
} from './flamebearer';

// Mirrors what the backend emits for main → {a:15, b:7} (widest first).
const SINGLE: Flamebearer = {
  names: ['total', 'main', 'a', 'b'],
  levels: [
    [0, 22, 0, 0],
    [0, 22, 0, 1],
    [0, 15, 15, 2, 0, 7, 7, 3],
  ],
  numTicks: 22,
  maxSelf: 15,
  units: 'nanoseconds',
};

describe('decodeNodes', () => {
  it('resolves relative offsets into absolute starts', () => {
    const nodes = decodeNodes(SINGLE.levels, false);
    // depth 2 holds a(start 0, total 15) then b(offset 0 → start 15, total 7).
    const a = nodes.find((n) => n.nameIndex === 2)!;
    const b = nodes.find((n) => n.nameIndex === 3)!;
    expect(a.start).toBe(0);
    expect(a.total).toBe(15);
    expect(b.start).toBe(15);
    expect(b.total).toBe(7);
  });

  it('decodes the trailing delta for diff bars', () => {
    const diff: DiffFlamebearer = {
      names: ['total', 'main', 'hot'],
      levels: [
        [0, 40, 0, 0, 20],
        [0, 40, 0, 1, 20],
        [0, 40, 40, 2, 20],
      ],
      numTicks: 40,
      maxSelf: 40,
      maxAbsDelta: 20,
      units: 'nanoseconds',
    };
    const nodes = decodeNodes(diff.levels, true);
    expect(nodes.find((n) => n.nameIndex === 2)?.delta).toBe(20);
  });
});

describe('zoom containment', () => {
  it('keeps nodes inside the focus window and finds ancestors', () => {
    const nodes = decodeNodes(SINGLE.levels, false);
    const b = nodes.find((n) => n.nameIndex === 3)!; // start 15, total 7
    const win = { depth: b.depth, start: b.start, total: b.total };
    // "a" (0..15) is disjoint from the b window (15..22).
    const a = nodes.find((n) => n.nameIndex === 2)!;
    expect(nodeInWindow(a, win)).toBe(false);
    expect(nodeInWindow(b, win)).toBe(true);
    // main (full width) is the ancestor at depth 1.
    expect(ancestorAtDepth(nodes, win, 1)?.nameIndex).toBe(1);
  });
});

describe('diff helpers', () => {
  it('classifies tone by delta sign', () => {
    expect(diffTone(5)).toBe('increase');
    expect(diffTone(-5)).toBe('decrease');
    expect(diffTone(0)).toBe('neutral');
    expect(diffTone(undefined)).toBe('neutral');
  });

  it('normalizes intensity against the max absolute delta', () => {
    expect(diffIntensity(10, 20)).toBe(0.5);
    expect(diffIntensity(40, 20)).toBe(1);
    expect(diffIntensity(0, 20)).toBe(0);
  });
});

describe('formatting', () => {
  it('formats durations and bytes by magnitude', () => {
    expect(formatDuration(500)).toBe('500 ns');
    expect(formatDuration(1_500_000)).toBe('1.5 ms');
    expect(formatBytes(2048)).toBe('2.0 KiB');
  });

  it('writes a duration in the unit it is read in, without decimals it does not have', () => {
    expect(formatDuration(31_850_000_000)).toBe('31.85 s');
    expect(formatDuration(1_500_000_000)).toBe('1.5 s');
    expect(formatDuration(2_000_000)).toBe('2 ms');
    // A minute of capture, and forty-eight of them.
    expect(formatDuration(60_000_000_000)).toBe('1 min');
    expect(formatDuration(2_880_000_000_000)).toBe('48 min');
    expect(formatDuration(90_000_000_000)).toBe('1.5 min');
    expect(formatDuration(7_200_000_000_000)).toBe('2 h');
  });

  it('falls back to levels[0] total when numTicks is absent', () => {
    expect(rootTotal({ ...SINGLE, numTicks: 0 })).toBe(22);
  });
});

describe('topFunctions', () => {
  it('aggregates self time by name, sorted desc, dropping the root', () => {
    const top = topFunctions(SINGLE);
    expect(top[0]).toMatchObject({ name: 'a', self: 15 });
    expect(top.find((f) => f.name === 'b')?.self).toBe(7);
    expect(top.map((f) => f.name)).not.toContain('total');
    expect(top[0]!.selfPct).toBeCloseTo((15 / 22) * 100, 1);
    expect(top[0]!.totalPct).toBeCloseTo((15 / 22) * 100, 1);
  });
});

describe('diffFunctions', () => {
  // main calls a and b. Both periods summed: a 30, b 10. The second period has 10 more of a
  // and 4 less of b than the first, so main as a whole has 6 more.
  const DIFF: DiffFlamebearer = {
    names: ['total', 'main', 'a', 'b', 'c'],
    levels: [
      [0, 44, 0, 0, 6],
      [0, 44, 4, 1, 6],
      [0, 30, 30, 2, 10, 0, 10, 10, 3, -4],
    ],
    numTicks: 44,
    maxSelf: 30,
    maxAbsDelta: 10,
    units: 'nanoseconds',
  };

  it('lists the functions by how much their own cost changed, largest change first', () => {
    const changed = diffFunctions(DIFF);

    expect(changed.map((entry) => entry.name)).toEqual(['a', 'b']);
    // a: 30 in both, 10 more in the second: 10 before, 20 after, twice as much.
    expect(changed[0]).toEqual({ name: 'a', baseline: 10, comparison: 20, delta: 10, deltaPct: 100 });
    // b: 10 in both, 4 less in the second: 7 before, 3 after.
    expect(changed[1]!.baseline).toBe(7);
    expect(changed[1]!.comparison).toBe(3);
    expect(changed[1]!.delta).toBe(-4);
    expect(changed[1]!.deltaPct).toBeCloseTo(-57.14, 1);
  });

  it('leaves out what did not change of its own, and the root', () => {
    // main is 6 more only because of what it calls: its own 4 is the same in both periods.
    expect(diffFunctions(DIFF).map((entry) => entry.name)).not.toContain('main');
    expect(diffFunctions(DIFF).map((entry) => entry.name)).not.toContain('total');
  });

  it('has no share to give for a function that did not run in the baseline', () => {
    const fresh: DiffFlamebearer = {
      ...DIFF,
      levels: [
        [0, 8, 0, 0, 8],
        [0, 8, 8, 4, 8],
      ],
    };

    expect(diffFunctions(fresh)).toEqual([
      { name: 'c', baseline: 0, comparison: 8, delta: 8, deltaPct: null },
    ]);
  });
});

describe('diffTotals', () => {
  it('tells the two periods apart from their sum and their difference', () => {
    // Both periods add up to 44, and the second has 6 more than the first: 19 and 25.
    const totals = diffTotals({
      names: ['total', 'main'],
      levels: [
        [0, 44, 0, 0, 6],
        [0, 44, 44, 1, 6],
      ],
      numTicks: 44,
      maxSelf: 44,
      maxAbsDelta: 6,
      units: 'nanoseconds',
    });

    expect(totals.baseline).toBe(19);
    expect(totals.comparison).toBe(25);
    expect(totals.delta).toBe(6);
    expect(totals.deltaPct).toBeCloseTo(31.58, 1);
  });

  it('has no share to give when the baseline is empty', () => {
    const totals = diffTotals({
      names: ['total', 'main'],
      levels: [
        [0, 8, 0, 0, 8],
        [0, 8, 8, 1, 8],
      ],
      numTicks: 8,
      maxSelf: 8,
      maxAbsDelta: 8,
      units: 'nanoseconds',
    });

    expect(totals).toEqual({ baseline: 0, comparison: 8, delta: 8, deltaPct: null });
  });
});

describe('a change, as it is written', () => {
  it('carries its direction', () => {
    expect(formatChange(1_200_000_000, 'nanoseconds')).toBe('+1.2 s');
    expect(formatChange(-340_000_000, 'nanoseconds')).toBe('-340 ms');
    expect(formatChange(0, 'nanoseconds')).toBe('0 ns');
    expect(formatChange(2048, 'bytes')).toBe('+2.0 KiB');
  });

  it('is a share with a decimal while it is small and without one once it is large', () => {
    expect(formatChangePct(11.24)).toBe('+11.2%');
    expect(formatChangePct(-3.4)).toBe('-3.4%');
    expect(formatChangePct(25)).toBe('+25%');
    expect(formatChangePct(283.4)).toBe('+283%');
    expect(formatChangePct(0)).toBe('0%');
  });
});

describe('the root frame', () => {
  it('is shown as the start of a path, not under the name of a measure', () => {
    expect(isRootFrame('total', 0)).toBe(true);
    expect(frameLabel('total', 0)).toBe('/');
  });

  it('leaves a function its name, one called total included', () => {
    expect(frameLabel('runtime.main', 1)).toBe('runtime.main');
    expect(isRootFrame('total', 3)).toBe(false);
    expect(frameLabel('total', 3)).toBe('total');
    expect(frameLabel('main', 0)).toBe('main');
  });
});

describe('heatColor', () => {
  it('ramps cool → hot and clamps out-of-range ratios', () => {
    expect(heatColor(0)).toBe('rgb(56, 130, 184)');
    expect(heatColor(1)).toBe('rgb(214, 78, 56)');
    expect(heatColor(2)).toBe('rgb(214, 78, 56)');
    expect(heatColor(-1)).toBe('rgb(56, 130, 184)');
  });
});

describe('unitForProfileType', () => {
  it('maps profile types to value units', () => {
    expect(unitForProfileType('cpu')).toBe('nanoseconds');
    expect(unitForProfileType('alloc_space')).toBe('bytes');
    expect(unitForProfileType('goroutines')).toBe('count');
    expect(unitForProfileType('mystery')).toBe('samples');
  });
});

describe('callTreeChildren', () => {
  it('reconstructs parent → children from span containment', () => {
    const nodes = decodeNodes(SINGLE.levels, false);
    const tree = callTreeChildren(nodes);
    const nameOf = (n: { nameIndex: number }) => SINGLE.names[n.nameIndex];
    expect((tree.get('root') ?? []).map(nameOf)).toEqual(['total']);
    const total = tree.get('root')![0]!;
    expect((tree.get(nodeKey(total)) ?? []).map(nameOf)).toEqual(['main']);
    const main = tree.get(nodeKey(total))![0]!;
    // widest-first: a(15) before b(7)
    expect((tree.get(nodeKey(main)) ?? []).map(nameOf)).toEqual(['a', 'b']);
  });
});
