import { describe, expect, it } from 'vitest';

import type { Span, TraceResponse } from '@/api/web';

import { buildTraceTree, pathTo } from '../../model/tree';
import { checkoutTrace, span, T0, trace } from './fixture';

const MS = 1_000_000;

describe('buildTraceTree', () => {
  it('lays the spans out each after the span that called it, with depths and offsets', () => {
    const tree = buildTraceTree(checkoutTrace())!;

    expect(tree.nodes.map((node) => `${node.id}@${node.depth}`)).toEqual(['root@0', 'inv@1', 'q1@2', 'q2@2', 'pay@1']);
    expect(tree.root.id).toBe('root');
    expect(tree.startNs).toBe(T0);
    expect(tree.totalNs).toBe(100 * MS);
    expect(tree.byId.get('pay')!.offsetNs).toBe(55 * MS);
    expect(tree.byId.get('root')!.descendants).toBe(4);
    expect(tree.byId.get('inv')!.descendants).toBe(2);
    expect(tree.maxDepth).toBe(2);
    expect(tree.errorCount).toBe(1);
  });

  it('counts calls that overlap once when working out a span’s own time', () => {
    const tree = buildTraceTree(checkoutTrace())!;

    // inventory runs 40 ms; its two queries cover 12–40 ms, 28 ms of it.
    expect(tree.byId.get('inv')!.selfNs).toBe(12 * MS);
    // checkout runs 100 ms; inventory and payments cover 80 ms of it.
    expect(tree.byId.get('root')!.selfNs).toBe(20 * MS);
    expect(tree.byId.get('q1')!.selfNs).toBe(18 * MS);
  });

  it('gives each service of the trace its own colour, in the order the trace enters it', () => {
    const tree = buildTraceTree(checkoutTrace())!;

    expect(tree.services.map((service) => [service.name, service.color])).toEqual([
      ['checkout', 'var(--chart-1)'],
      ['inventory', 'var(--chart-2)'],
      ['postgres', 'var(--chart-3)'],
      ['payments', 'var(--chart-4)'],
    ]);
    expect(tree.services.reduce((total, service) => total + service.share, 0)).toBeCloseTo(1);
    expect(tree.services.find((service) => service.name === 'payments')!.errors).toBe(1);
  });

  it('keeps a span whose parent never arrived as a root of its own instead of failing the trace', () => {
    const tree = buildTraceTree(
      trace([
        span('late', 'missing', 'worker', 'job.step', 30, 40),
        span('root', null, 'worker', 'job', 0, 20),
      ], 'root'),
    )!;

    expect(tree.roots.map((node) => node.id)).toEqual(['root', 'late']);
    expect(tree.byId.get('late')!.orphan).toBe(true);
    expect(tree.orphanCount).toBe(1);
    expect(tree.totalNs).toBe(40 * MS);
  });

  it('stands spans whose parents loop as roots, so none is left out', () => {
    const tree = buildTraceTree(trace([span('a', 'b', 'x', 'a', 0, 1), span('b', 'a', 'x', 'b', 0, 1)]))!;

    expect(tree.nodes).toHaveLength(2);
    expect(tree.roots.length).toBeGreaterThan(0);
  });

  it('has no tree for a trace without spans, and drops a span received twice', () => {
    expect(buildTraceTree(trace([]))).toBeNull();

    const twice = buildTraceTree(trace([span('a', null, 'x', 'a', 0, 5), span('a', null, 'x', 'a', 0, 5)]))!;
    expect(twice.nodes).toHaveLength(1);
  });

  it('leads from the root down to a span', () => {
    const tree = buildTraceTree(checkoutTrace())!;
    expect(pathTo(tree, 'q2').map((node) => node.id)).toEqual(['root', 'inv', 'q2']);
  });
});

describe('a span an older or partial answer left short', () => {
  it('is read with the lists and maps it lacks, empty, and a length from its end', () => {
    const tree = buildTraceTree({
      traceId: 'short',
      rootSpanId: 'root',
      spans: [{ spanId: 'root', service: 'web', operation: 'GET /', startNs: 1_000, endNs: 5_000, status: 'OK' } as unknown as Span],
    } as unknown as TraceResponse)!;

    const root = tree.root;
    expect(root.span.links).toEqual([]);
    expect(root.span.events).toEqual([]);
    expect(root.span.attributes).toEqual({});
    expect(root.span.resource.attributes).toEqual({});
    expect(root.durationNs).toBe(4_000);
  });
});
