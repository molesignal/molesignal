import { describe, expect, it } from 'vitest';

import { criticalPath } from '../../model/criticalPath';
import { buildTraceTree } from '../../model/tree';
import { checkoutTrace, span, T0, trace } from './fixture';

const MS = 1_000_000;
const offsets = (path: ReturnType<typeof criticalPath>) =>
  path.segments.map((segment) => `${segment.id} ${(segment.fromNs - T0) / MS}-${(segment.toNs - T0) / MS}`);

describe('criticalPath', () => {
  it('walks back from the root’s end through the child that ended last, giving the rest to the caller', () => {
    const path = criticalPath(buildTraceTree(checkoutTrace())!);

    // The payment ends last; before it the stock lookup, whose second query ended last. The first query
    // was still running when the second began: it held nothing up.
    expect(offsets(path)).toEqual([
      'root 0-10',
      'inv 10-20',
      'q2 20-40',
      'inv 40-50',
      'root 50-55',
      'pay 55-95',
      'root 95-100',
    ]);
    expect(path.totalNs).toBe(100 * MS);
    expect([...path.spans].sort()).toEqual(['inv', 'pay', 'q2', 'root']);
    expect(path.nsById.get('root')).toBe(20 * MS);
    expect(path.segmentsById.get('inv')!.map((segment) => (segment.toNs - segment.fromNs) / MS)).toEqual([10, 10]);
  });

  it('says how much of the path each service holds, the most first', () => {
    const path = criticalPath(buildTraceTree(checkoutTrace())!);

    expect(path.services.map((service) => [service.name, Math.round(service.share * 100)])).toEqual([
      ['payments', 40],
      ['checkout', 20],
      ['inventory', 20],
      ['postgres', 20],
    ]);
  });

  it('leaves out calls that ran alongside the one the trace waited for', () => {
    const path = criticalPath(buildTraceTree(trace([
      span('root', null, 'web', 'GET /', 0, 100),
      span('slow', 'root', 'api', 'GET /slow', 5, 90),
      span('fast', 'root', 'api', 'GET /fast', 5, 40),
      // Sent without waiting for it, before the slow call began.
      span('publish', 'root', 'queue', 'publish', 2, 30),
    ]))!);

    expect(offsets(path)).toEqual(['root 0-5', 'slow 5-90', 'root 90-100']);
    expect(path.spans.has('fast')).toBe(false);
    expect(path.spans.has('publish')).toBe(false);
    expect(path.services.map((service) => service.name)).toEqual(['api', 'web']);
  });

  it('follows calls made one after another, back to back', () => {
    const path = criticalPath(buildTraceTree(trace([
      span('root', null, 'web', 'GET /', 0, 60),
      span('first', 'root', 'api', 'GET /a', 0, 20),
      span('second', 'root', 'api', 'GET /b', 20, 60),
    ]))!);

    expect(offsets(path)).toEqual(['first 0-20', 'second 20-60']);
  });

  it('cuts a child to its caller’s span, before and after it', () => {
    const path = criticalPath(buildTraceTree(trace([
      span('root', null, 'web', 'GET /', 10, 100),
      span('early', 'root', 'api', 'GET /early', 0, 30),
      span('late', 'root', 'queue', 'publish', 80, 400),
    ]))!);

    expect(offsets(path)).toEqual(['early 10-30', 'root 30-80', 'late 80-100']);
    expect(path.totalNs).toBe(90 * MS);
  });

  it('walks a chain deeper than the call stack', () => {
    const depth = 20_000;
    const spans = Array.from({ length: depth }, (_, index) =>
      span(`s${index}`, index === 0 ? null : `s${index - 1}`, 'deep', `call ${index}`, index * 0.001, 100),
    );
    const path = criticalPath(buildTraceTree(trace(spans))!);

    expect(path.spans.size).toBe(depth);
    expect(path.totalNs).toBeCloseTo(100 * MS, -3);
  });
});
