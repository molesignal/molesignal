import { formatMicrosActive } from '@/lib/time';

import type { LiveTool } from '../run/runState';

/** Calls of one tool in an answer, folded into one row. */
export interface AggregatedTool {
  name: string;
  count: number;
  status: LiveTool['status'];
  calls: LiveTool[];
}


export type ToolPayloadKind = 'arguments' | 'result' | 'error';

export function aggregateTools(tools: LiveTool[]): AggregatedTool[] {
  const map = new Map<string, AggregatedTool>();
  for (const tool of tools) {
    const existing = map.get(tool.name);
    if (!existing) {
      map.set(tool.name, { name: tool.name, count: 1, status: tool.status, calls: [tool] });
      continue;
    }
    existing.count += 1;
    existing.status = mergeToolStatus(existing.status, tool.status);
    existing.calls.push(tool);
  }
  return [...map.values()];
}

function mergeToolStatus(a: LiveTool['status'], b: LiveTool['status']): LiveTool['status'] {
  if (a === 'running' || b === 'running') return 'running';
  if (a === 'error' || b === 'error') return 'error';
  return 'done';
}

export function summarizeToolPayload(
  toolName: string,
  value: string | undefined,
  kind: ToolPayloadKind,
): string[] {
  const trimmed = value?.trim() ?? '';
  if (!trimmed) return [];
  const parsed = parseJson(trimmed);
  if (parsed === null) return [capToolLine(trimmed)];

  const payloads = unwrapToolPayload(parsed);
  if (toolName === 'list_streams' && kind !== 'arguments') {
    return collectStreamNames(payloads).map(capToolLine);
  }
  if (kind === 'arguments') {
    return summarizeArguments(payloads);
  }
  return summarizeGenericPayload(payloads);
}

export function parseJson(value: string): unknown | null {
  try {
    return JSON.parse(value) as unknown;
  } catch {
    return null;
  }
}

function unwrapToolPayload(value: unknown): unknown[] {
  if (Array.isArray(value)) {
    return value.flatMap((item) => {
      if (!isRecord(item)) return [item];
      if (item.type === 'json') return unwrapToolPayload(item.json);
      if (item.type === 'text') return typeof item.text === 'string' ? [item.text] : [];
      return [item];
    });
  }
  if (isRecord(value) && Array.isArray(value.content)) {
    return unwrapToolPayload(value.content);
  }
  return [value];
}

function summarizeArguments(payloads: unknown[]): string[] {
  return payloads.flatMap((payload) => {
    if (!isRecord(payload)) return summarizePrimitive(payload);
    return Object.entries(payload).flatMap(([key, value]) => summarizeField(key, value));
  });
}

function summarizeGenericPayload(payloads: unknown[]): string[] {
  const lines = payloads.flatMap((payload) => {
    if (!isRecord(payload)) return summarizePrimitive(payload);
    const countLines = ['rows', 'incidents', 'streams', 'spans', 'alerts']
      .flatMap((key) => {
        const value = payload[key];
        return Array.isArray(value) ? [`${key}: ${value.length}`] : [];
      });
    const fieldLines = Object.entries(payload)
      .filter(([key]) => !['rows', 'incidents', 'streams', 'spans', 'alerts'].includes(key))
      .flatMap(([key, value]) => summarizeField(key, value));
    return [...countLines, ...fieldLines];
  });
  return uniqueLines(lines.map(capToolLine));
}

function summarizeField(key: string, value: unknown): string[] {
  if (value === null || value === undefined || value === '') return [];
  if (key === 'time_range' && isRecord(value)) {
    const start = formatMicros(value.start_micros);
    const end = formatMicros(value.end_micros);
    if (start || end) return [`time_range: ${start || '?'} - ${end || '?'}`];
  }
  if (Array.isArray(value)) {
    if (value.length === 0) return [`${key}: 0`];
    const primitiveValues = value
      .map((item) => (typeof item === 'string' || typeof item === 'number' ? String(item) : null))
      .filter((item): item is string => Boolean(item));
    if (primitiveValues.length === value.length && primitiveValues.length <= 6) {
      return [`${key}: ${primitiveValues.join(', ')}`];
    }
    return [`${key}: ${value.length}`];
  }
  if (typeof value === 'string' || typeof value === 'number' || typeof value === 'boolean') {
    return [`${key}: ${String(value)}`];
  }
  return [];
}

function summarizePrimitive(value: unknown): string[] {
  if (typeof value === 'string') return value.trim() ? [value.trim()] : [];
  if (typeof value === 'number' || typeof value === 'boolean') return [String(value)];
  return [];
}

function collectStreamNames(payloads: unknown[]): string[] {
  const names = payloads.flatMap((payload) => collectStreamNamesFromValue(payload));
  return uniqueLines(names);
}

function collectStreamNamesFromValue(value: unknown): string[] {
  if (typeof value === 'string') return [value];
  if (Array.isArray(value)) return value.flatMap((item) => collectStreamNamesFromValue(item));
  if (!isRecord(value)) return [];
  const direct = typeof value.name === 'string'
    ? [value.name]
    : typeof value.stream === 'string'
      ? [value.stream]
      : typeof value.stream_name === 'string'
        ? [value.stream_name]
        : [];
  const nested = ['streams', 'items', 'data', 'results']
    .flatMap((key) => collectStreamNamesFromValue(value[key]));
  return [...direct, ...nested];
}

function formatMicros(value: unknown): string | null {
  if (typeof value !== 'number' || !Number.isFinite(value)) return null;
  // Tool payloads carry epoch micros (recent times exceed 1e13); older callers
  // may pass millis. Normalize to micros, then render via the central tz layer.
  const micros = value > 10_000_000_000_000 ? value : value * 1000;
  return formatMicrosActive(micros);
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function uniqueLines(lines: string[]): string[] {
  return [...new Set(lines.map((line) => line.trim()).filter(Boolean))];
}

function capToolLine(line: string): string {
  return line.length <= 180 ? line : `${line.slice(0, 180)}...`;
}

