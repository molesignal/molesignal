import type { Span, TraceResponse } from '@/api/web';

import type { LaidOutTrace, RenderMode, SpanNode } from './types';

export interface LayoutError {
  kind: 'multiple_roots' | 'no_root' | 'empty';
  rootCount?: number;
}

export type LayoutResult = { ok: true; data: LaidOutTrace } | { ok: false; error: LayoutError };

/**
 * Build the span tree + per-node depth + offset/duration in O(n). Reject the
 * trace if it has zero or more than one parentless span — that signals a
 * malformed export and downstream rendering would silently truncate.
 */
export function layoutTrace(trace: TraceResponse, mode: RenderMode): LayoutResult {
  if (trace.spans.length === 0) return { ok: false, error: { kind: 'empty' } };

  const byId = new Map<string, number>();
  for (let i = 0; i < trace.spans.length; i++) byId.set(trace.spans[i]!.span_id, i);

  const roots: number[] = [];
  for (const s of trace.spans) {
    if (!s.parent_span_id || !byId.has(s.parent_span_id)) roots.push(byId.get(s.span_id)!);
  }
  if (roots.length === 0) return { ok: false, error: { kind: 'no_root' } };
  if (roots.length > 1) return { ok: false, error: { kind: 'multiple_roots', rootCount: roots.length } };

  const rootIdx = roots[0]!;
  const rootSpan = trace.spans[rootIdx]!;
  const rootStart = rootSpan.start_ns;

  // BFS for depth + child lists
  const depths = new Array<number>(trace.spans.length).fill(0);
  const childIds: string[][] = trace.spans.map(() => []);
  for (const s of trace.spans) {
    if (s.parent_span_id && byId.has(s.parent_span_id)) {
      childIds[byId.get(s.parent_span_id)!]!.push(s.span_id);
    }
  }
  // Compute depths iteratively (avoid recursion blow on 100k spans).
  const stack: Array<{ idx: number; depth: number }> = [{ idx: rootIdx, depth: 0 }];
  while (stack.length > 0) {
    const { idx, depth } = stack.pop()!;
    depths[idx] = depth;
    for (const cId of childIds[idx]!) {
      stack.push({ idx: byId.get(cId)!, depth: depth + 1 });
    }
  }

  // Waterfall: sort all spans by start_ns to assign a row index.
  const sortedForWf = trace.spans
    .map((s, i) => ({ s, i }))
    .sort((a, b) => a.s.start_ns - b.s.start_ns || depths[a.i]! - depths[b.i]!);
  const rowOf = new Array<number>(trace.spans.length);
  for (let r = 0; r < sortedForWf.length; r++) rowOf[sortedForWf[r]!.i] = r;

  const nodes: SpanNode[] = trace.spans.map((s, i) => ({
    span: s,
    depth: depths[i]!,
    startOffsetNs: s.start_ns - rootStart,
    durationNs: Math.max(1, s.end_ns - s.start_ns),
    childIds: childIds[i]!,
    rowIndex: mode === 'flame' ? depths[i]! : rowOf[i]!,
  }));

  let maxDepth = 0;
  let latestEnd = 0;
  for (const s of trace.spans) {
    const d = depths[byId.get(s.span_id)!]!;
    if (d > maxDepth) maxDepth = d;
    if (s.end_ns - rootStart > latestEnd) latestEnd = s.end_ns - rootStart;
  }

  return {
    ok: true,
    data: {
      trace,
      nodes,
      totalDurationNs: latestEnd,
      maxDepth,
      rowCount: trace.spans.length,
      byId,
    },
  };
}

export function spanFromId(layout: LaidOutTrace, spanId: string): Span | undefined {
  const idx = layout.byId.get(spanId);
  return idx == null ? undefined : layout.nodes[idx]?.span;
}
