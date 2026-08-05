export type LogResultDensity = 'compact' | 'normal' | 'comfortable';
export type LogLevel = 'INFO' | 'WARN' | 'ERROR' | 'DEBUG' | 'TRACE';

export interface LogEntry {
  ts: string;
  level: LogLevel;
  raw: Record<string, unknown>;
}

export interface LogTopValue {
  value: unknown;
  label: string;
  count: number;
}

const MESSAGE_FIELD_PRIORITY = [
  'message',
  'body',
  'summary',
  'error',
  'event',
  'log',
  'model',
] as const;

const SOURCE_FIELD_PRIORITY = [
  'service.name',
  'service_name',
  'service',
  'source',
  'provider',
  'model',
  'host',
] as const;

const LEVEL_FIELD_PRIORITY = [
  'level',
  'severity_text',
  'severity',
  'log.level',
] as const;

const TABLE_FIELD_PRIORITY = [
  'level',
  'service.name',
  'service_name',
  'service',
  'source',
  'message',
  'body',
  'model',
  'provider',
  'error',
  'trace_id',
  'span_id',
] as const;

const TIMESTAMP_FIELDS = new Set(['_timestamp', 'timestamp', 'time']);

function isPresent(value: unknown): boolean {
  return value !== null && value !== undefined && value !== '';
}

function readRecordValue(record: Record<string, unknown>, path: string): unknown {
  if (Object.prototype.hasOwnProperty.call(record, path)) return record[path];
  let current: unknown = record;
  for (const part of path.split('.')) {
    if (!current || typeof current !== 'object' || !Object.prototype.hasOwnProperty.call(current, part)) {
      return undefined;
    }
    current = (current as Record<string, unknown>)[part];
  }
  return current;
}

export function displayLogValue(value: unknown): string {
  if (!isPresent(value)) return '';
  if (typeof value === 'string') return value;
  if (typeof value === 'number' || typeof value === 'boolean') return String(value);
  try {
    return JSON.stringify(value) ?? String(value);
  } catch {
    return String(value);
  }
}

function firstPresentValue(
  record: Record<string, unknown>,
  fields: readonly string[],
): { field: string; value: unknown } | null {
  for (const field of fields) {
    const value = readRecordValue(record, field);
    if (isPresent(value)) return { field, value };
  }
  return null;
}

export function primaryLogMessage(record: Record<string, unknown>): {
  field: string;
  value: string;
} {
  const preferred = firstPresentValue(record, MESSAGE_FIELD_PRIORITY);
  if (preferred) {
    return { field: preferred.field, value: displayLogValue(preferred.value) };
  }

  const fallback = Object.entries(record).find(([field, value]) => (
    !TIMESTAMP_FIELDS.has(field) && isPresent(value)
  ));
  if (fallback) return { field: fallback[0], value: displayLogValue(fallback[1]) };
  return { field: '', value: '' };
}

export function logSourceLabel(record: Record<string, unknown>): string {
  const source = firstPresentValue(record, SOURCE_FIELD_PRIORITY);
  return source ? displayLogValue(source.value) : '—';
}

export function logLevelLabel(record: Record<string, unknown>, fallback = 'INFO'): string {
  const level = firstPresentValue(record, LEVEL_FIELD_PRIORITY);
  return (level ? displayLogValue(level.value) : fallback).toUpperCase();
}

export function defaultLogTableFields(fieldNames: string[], limit = 8): string[] {
  const available = fieldNames.filter((field) => !TIMESTAMP_FIELDS.has(field));
  const preferred = TABLE_FIELD_PRIORITY.filter((field) => available.includes(field));
  const preferredSet = new Set<string>(preferred);
  const fallback = available.filter((field) => !preferredSet.has(field));
  return [...preferred, ...fallback].slice(0, limit);
}

export function topLogFieldValues(
  records: Record<string, unknown>[],
  field: string,
  limit = 5,
): LogTopValue[] {
  const counts = new Map<string, { value: unknown; count: number }>();
  for (const record of records) {
    const value = readRecordValue(record, field);
    if (!isPresent(value)) continue;
    const label = displayLogValue(value);
    if (!label) continue;
    const key = `${typeof value}:${label}`;
    const existing = counts.get(key);
    if (existing) existing.count += 1;
    else counts.set(key, { value, count: 1 });
  }
  return Array.from(counts.values())
    .map(({ value, count }) => ({ value, count, label: displayLogValue(value) }))
    .sort((left, right) => right.count - left.count || left.label.localeCompare(right.label))
    .slice(0, limit);
}

export function logResultRowHeight(density: LogResultDensity): number {
  if (density === 'compact') return 32;
  if (density === 'comfortable') return 48;
  return 40;
}

export function recordsToCsv(records: Record<string, unknown>[], columns: string[]): string {
  const escapeCell = (value: unknown): string => {
    const rendered = displayLogValue(value);
    return `"${rendered.replace(/"/g, '""')}"`;
  };
  const header = columns.map(escapeCell).join(',');
  const rows = records.map((record) => (
    columns.map((column) => escapeCell(readRecordValue(record, column))).join(',')
  ));
  return [header, ...rows].join('\n');
}

function logTextValue(value: unknown): string {
  const rendered = displayLogValue(value);
  if (!rendered) return '';
  return /^[^\s="\\]+$/.test(rendered) ? rendered : JSON.stringify(rendered);
}

export function recordsToLogText(records: Record<string, unknown>[], columns: string[]): string {
  return records.map((record) => (
    columns.flatMap((column) => {
      const rendered = logTextValue(readRecordValue(record, column));
      if (!rendered) return [];
      return TIMESTAMP_FIELDS.has(column) ? [rendered] : [`${column}=${rendered}`];
    }).join(' ')
  )).join('\n');
}
