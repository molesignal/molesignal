import type { Span } from '@/api/web';
import type { SignalReferenceTime } from '@/shell/SignalReference';

import type { LaidOutTrace, SpanNode } from './types';
interface TraceRow { node: SpanNode; children: SpanNode[]; }

const TRACE_CONTEXT_PADDING_MS = 5 * 60 * 1000;

export function traceContextWindow(traceStartNs: number, totalNs: number): SignalReferenceTime | undefined {
  const startMs = traceStartNs / 1_000_000;
  const endMs = (traceStartNs + totalNs) / 1_000_000;
  if (!Number.isFinite(startMs) || !Number.isFinite(endMs) || startMs <= 0 || endMs < startMs) {
    return undefined;
  }
  const from = new Date(startMs - TRACE_CONTEXT_PADDING_MS);
  const to = new Date(endMs + TRACE_CONTEXT_PADDING_MS);
  if (Number.isNaN(from.getTime()) || Number.isNaN(to.getTime())) return undefined;
  return { from: from.toISOString(), to: to.toISOString() };
}

export function signalLabelsForTrace(traceId: string, span: Span): Record<string, string> {
  return {
    ...stringSignalAttributes(span.attributes),
    trace_id: traceId,
    service_name: span.service,
    operation_name: span.operation,
  };
}

export function signalLabelsForSpan(traceId: string, span: Span): Record<string, string> {
  return {
    ...signalLabelsForTrace(traceId, span),
    span_id: span.span_id,
  };
}

function stringSignalAttributes(
  attributes: Record<string, unknown>,
  prefix = '',
  depth = 0,
): Record<string, string> {
  const labels: Record<string, string> = {};
  for (const [key, value] of Object.entries(attributes)) {
    const qualifiedKey = prefix ? `${prefix}.${key}` : key;
    if (typeof value === 'string' || typeof value === 'number' || typeof value === 'boolean') {
      labels[qualifiedKey] = String(value);
      continue;
    }
    if (depth < 1 && value && typeof value === 'object' && !Array.isArray(value)) {
      Object.assign(
        labels,
        stringSignalAttributes(value as Record<string, unknown>, qualifiedKey, depth + 1),
      );
    }
  }
  return labels;
}

export function flattenTrace(layout: LaidOutTrace, collapsed: Set<string>): TraceRow[] {
  const byId = new Map(layout.nodes.map((node) => [node.span.span_id, node]));
  const children = new Map<string, SpanNode[]>();
  const roots: SpanNode[] = [];

  for (const node of layout.nodes) {
    const parentId = node.span.parent_span_id;
    if (parentId && byId.has(parentId)) {
      const list = children.get(parentId) ?? [];
      list.push(node);
      children.set(parentId, list);
    } else {
      roots.push(node);
    }
  }
  for (const list of children.values()) {
    list.sort((a, b) => a.startOffsetNs - b.startOffsetNs || a.span.operation.localeCompare(b.span.operation));
  }
  roots.sort((a, b) => a.startOffsetNs - b.startOffsetNs);

  const rows: TraceRow[] = [];
  const walk = (node: SpanNode) => {
    const childRows = children.get(node.span.span_id) ?? [];
    rows.push({ node, children: childRows });
    if (collapsed.has(node.span.span_id)) return;
    for (const child of childRows) walk(child);
  };
  for (const root of roots) walk(root);
  return rows;
}

export function pct(value: number, total: number): number {
  if (!Number.isFinite(value) || !Number.isFinite(total) || total <= 0) return 0;
  return Math.max(0, Math.min(100, (value / total) * 100));
}

export function formatJson(value: unknown): string {
  if (typeof value === 'string') {
    try {
      return JSON.stringify(JSON.parse(value), null, 2);
    } catch {
      return value;
    }
  }
  return JSON.stringify(value ?? {}, null, 2);
}
