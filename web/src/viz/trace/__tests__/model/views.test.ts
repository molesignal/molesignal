import { describe, expect, it } from 'vitest';

import { groupSpans } from '../../model/aggregate';
import { percentAcross, tickStep, timeTicks } from '../../model/axis';
import { foldableIds, foldedAbove, waterfallRows } from '../../model/rows';
import { parseSpanSearch, searchSpans } from '../../model/search';
import { traceServiceMap } from '../../model/serviceMap';
import { buildTraceTree } from '../../model/tree';
import { checkoutTrace } from './fixture';

const MS = 1_000_000;
const tree = () => buildTraceTree(checkoutTrace())!;
const ids = (rows: ReturnType<typeof waterfallRows>) => rows.map((row) => row.node.id);

describe('waterfallRows', () => {
  it('lists every span after its caller, and folds away what is under a folded span', () => {
    expect(ids(waterfallRows(tree(), { collapsed: new Set() }))).toEqual(['root', 'inv', 'q1', 'q2', 'pay']);

    const folded = waterfallRows(tree(), { collapsed: new Set(['inv']) });
    expect(ids(folded)).toEqual(['root', 'inv', 'pay']);
    expect(folded[1]).toMatchObject({ collapsed: true, hasChildren: true });
  });

  it('narrows to a span and what lies under it, counting depth from there', () => {
    const rows = waterfallRows(tree(), { collapsed: new Set(), focusId: 'inv' });
    expect(rows.map((row) => `${row.node.id}@${row.depth}`)).toEqual(['inv@0', 'q1@1', 'q2@1']);
  });

  it('keeps failed spans with the way down to them, marking the way as a path', () => {
    const rows = waterfallRows(tree(), { collapsed: new Set(), errorsOnly: true });

    expect(ids(rows)).toEqual(['root', 'pay']);
    expect(rows.map((row) => row.path)).toEqual([true, false]);
    expect(rows[0]!.hasChildren).toBe(true);
  });

  it('marks what a search found, and keeps only that when asked', () => {
    const matches = searchSpans(tree(), 'postgres');

    expect(waterfallRows(tree(), { collapsed: new Set(), matches }).filter((row) => row.matched).map((row) => row.node.id)).toEqual(['q1', 'q2']);
    expect(ids(waterfallRows(tree(), { collapsed: new Set(), matches, matchesOnly: true }))).toEqual(['root', 'inv', 'q1', 'q2']);
  });

  it('knows what folding all folds and what has to open for a span to be seen', () => {
    expect([...foldableIds(tree())].sort()).toEqual(['inv', 'root']);
    expect(foldedAbove(tree(), 'q2', new Set(['root', 'inv']))).toEqual(['inv', 'root']);
  });
});

describe('searchSpans', () => {
  it('finds words anywhere in a span, a tag holding a value, and durations compared', () => {
    expect([...searchSpans(tree(), 'stock')]).toEqual(['inv', 'q1', 'q2']);
    expect([...searchSpans(tree(), 'service:payments')]).toEqual(['pay']);
    expect([...searchSpans(tree(), 'http.route=/orders')]).toEqual(['root']);
    expect([...searchSpans(tree(), 'duration>=40ms')]).toEqual(['root', 'inv', 'pay']);
    expect([...searchSpans(tree(), 'status:error')]).toEqual(['pay']);
    expect(searchSpans(tree(), '  ').size).toBe(0);
  });

  it('keeps words in quotes together', () => {
    expect(parseSpanSearch('"POST /charge" duration<1s')).toEqual([
      { kind: 'text', text: 'post /charge' },
      { kind: 'duration', op: '<', ns: 1_000_000_000 },
    ]);
  });
});

describe('time axis', () => {
  it('marks round lengths of time, far enough apart to be read', () => {
    expect(tickStep(100 * MS, 800)).toBe(20 * MS);
    expect(timeTicks(0, 100 * MS, 800)).toEqual([0, 20 * MS, 40 * MS, 60 * MS, 80 * MS, 100 * MS]);
    expect(timeTicks(3 * MS, 50 * MS, 400)[0]).toBe(3 * MS);
  });

  it('places an offset across a window, clamped to its edges', () => {
    expect(percentAcross(25, 0, 100)).toBe(25);
    expect(percentAcross(150, 0, 100)).toBe(100);
    expect(percentAcross(5, 10, 10)).toBe(0);
  });
});

describe('groupSpans and traceServiceMap', () => {
  it('sums the spans by service and operation, the busiest first', () => {
    const groups = groupSpans(tree());

    expect(groups.map((group) => group.service)).toEqual(['payments', 'postgres', 'checkout', 'inventory']);
    const postgres = groups.find((group) => group.service === 'postgres')!;
    expect(postgres).toMatchObject({ spans: 2, totalNs: 38 * MS, avgNs: 19 * MS, selfNs: 38 * MS });
    expect(postgres.operations.map((operation) => operation.operation)).toEqual(['SELECT stock']);
    expect(groups.reduce((total, group) => total + group.share, 0)).toBeCloseTo(1);
  });

  it('draws a call wherever a span’s parent is in another service', () => {
    const map = traceServiceMap(tree());

    expect(map.services.find((service) => service.entry)!.name).toBe('checkout');
    expect(map.calls).toEqual([
      { from: 'checkout', to: 'inventory', calls: 1, errors: 0, durationNs: 40 * MS },
      { from: 'inventory', to: 'postgres', calls: 2, errors: 0, durationNs: 38 * MS },
      { from: 'checkout', to: 'payments', calls: 1, errors: 1, durationNs: 40 * MS },
    ]);
  });
});
