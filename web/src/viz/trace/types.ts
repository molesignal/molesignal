import type { Span, TraceResponse } from '@/api/web';

export type RenderMode = 'flame' | 'waterfall';

export interface SpanNode {
  span: Span;
  depth: number;
  /** ns since trace root start */
  startOffsetNs: number;
  /** ns duration */
  durationNs: number;
  childIds: string[];
  /** position assigned by layout (in row units) */
  rowIndex: number;
}

export interface LaidOutTrace {
  trace: TraceResponse;
  nodes: SpanNode[];
  /** total ns from earliest start to latest end */
  totalDurationNs: number;
  /** flame mode: max depth seen */
  maxDepth: number;
  /** waterfall mode: total rows = nodes.length */
  rowCount: number;
  /** map span_id → node index */
  byId: Map<string, number>;
}

export interface Viewport {
  /** seconds from trace start; left edge of visible window */
  fromNs: number;
  toNs: number;
  /** vertical offset in rows */
  scrollRow: number;
}
