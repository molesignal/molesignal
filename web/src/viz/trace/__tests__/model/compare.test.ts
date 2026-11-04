import { describe, expect, it } from 'vitest';

import { compareTraces } from '../../model/compare';
import { buildTraceTree } from '../../model/tree';
import { checkoutTrace, span, trace } from './fixture';

const MS = 1_000_000;

/** The checkout again, slower: the stock lookup runs one query more, and the payment retries through a fraud check. */
function slowerCheckout() {
  return trace([
    span('root', null, 'checkout', 'POST /orders', 0, 160),
    span('inv', 'root', 'inventory', 'GET /stock', 10, 70),
    span('q1', 'inv', 'postgres', 'SELECT stock', 12, 30),
    span('q2', 'inv', 'postgres', 'SELECT stock', 20, 40),
    span('q3', 'inv', 'postgres', 'SELECT stock', 40, 65),
    span('pay', 'root', 'payments', 'POST /charge', 75, 150),
    span('fraud', 'pay', 'fraud', 'POST /score', 80, 120),
    span('rules', 'fraud', 'fraud', 'load rules', 82, 90),
  ]);
}

describe('compareTraces', () => {
  const a = buildTraceTree(checkoutTrace())!;
  const b = buildTraceTree(slowerCheckout())!;
  const comparison = compareTraces(a, b);

  it('says how much longer the second trace took', () => {
    expect(comparison.durationDeltaNs).toBe(60 * MS);
  });

  it('sets the time of each service side by side, the biggest change first', () => {
    expect(comparison.services.map((row) => [row.service, row.a.spans, row.b.spans, row.deltaNs / MS])).toEqual([
      ['fraud', 0, 2, 40],
      ['postgres', 2, 3, 25],
      // The payment's own time fell: the fraud check now holds part of it. Of equal changes, the busier first.
      ['payments', 1, 1, -5],
      ['checkout', 1, 1, 5],
      ['inventory', 1, 1, -5],
    ]);
    expect(comparison.services.find((row) => row.service === 'payments')!.a).toEqual({ spans: 1, errors: 1, selfNs: 40 * MS });
  });

  it('does the same by operation', () => {
    const query = comparison.operations.find((row) => row.operation === 'SELECT stock')!;

    expect(query).toMatchObject({ service: 'postgres', a: { spans: 2, selfNs: 38 * MS }, b: { spans: 3, selfNs: 63 * MS } });
    expect(comparison.operations[0]).toMatchObject({ service: 'fraud', operation: 'POST /score' });
  });

  it('lists the calls only one trace made once, counting what they called under them', () => {
    expect(comparison.onlyInA).toEqual([]);
    expect(comparison.onlyInB.map((call) => [call.path.map((step) => step.operation).join(' > '), call.count, call.under])).toEqual([
      ['POST /orders > POST /charge > POST /score', 1, 1],
      ['POST /orders > GET /stock > SELECT stock', 1, 0],
    ]);
  });

  it('finds nothing missing between a trace and itself', () => {
    const same = compareTraces(a, buildTraceTree(checkoutTrace())!);

    expect(same.onlyInA).toEqual([]);
    expect(same.onlyInB).toEqual([]);
    expect(same.services.every((row) => row.deltaNs === 0)).toBe(true);
  });

  it('sets traces begun by different requests wholly apart', () => {
    const other = buildTraceTree(trace([span('r', null, 'checkout', 'GET /cart', 0, 20), span('c', 'r', 'redis', 'GET', 2, 4)]))!;
    const apart = compareTraces(a, other);

    expect(apart.onlyInA).toEqual([expect.objectContaining({ count: 1, under: 4 })]);
    expect(apart.onlyInB).toEqual([expect.objectContaining({ count: 1, under: 1 })]);
  });
});
