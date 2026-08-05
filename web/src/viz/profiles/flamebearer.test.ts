import { describe, expect, it } from 'vitest';

import type { DiffFlamebearer, Flamebearer } from '@/api/profiles';

import {
  ancestorAtDepth,
  callTreeChildren,
  decodeNodes,
  diffIntensity,
  diffTone,
  formatBytes,
  formatDuration,
  heatColor,
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
