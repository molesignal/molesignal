import { DATE_PATTERNS, zonedMoment, type DateFormat, type TimeFormat } from '@/lib/time';

export type LogLevel = 'INFO' | 'WARN' | 'ERROR' | 'DEBUG' | 'TRACE';

export interface LogEntry {
  /** The moment as the reader writes dates and times, to the second. */
  ts: string;
  /** The moment itself, in microseconds; `null` when the row carries no usable time. */
  micros: number | null;
  level: LogLevel;
  raw: Record<string, unknown>;
}

/** How loudly a level speaks: a failure, a warning, the ordinary run of things, or detail for a developer. */
export type LogLevelTone = 'error' | 'warn' | 'info' | 'quiet';

/** Levels are written many ways (ERR, error, Fatal, warning, dbg): the tone is read from how they begin. */
export function logLevelTone(level: string): LogLevelTone {
  const name = level.trim().toUpperCase();
  if (/^(ERR|FATAL|CRIT|EMERG|ALERT|PANIC|SEVERE)/.test(name)) return 'error';
  if (/^WARN/.test(name)) return 'warn';
  if (/^(DEBUG|DBG|TRACE|VERBOSE|FINE)/.test(name)) return 'quiet';

  return 'info';
}

/** How the reader has asked to see time. */
export interface LogClock {
  /** IANA name of the zone. */
  tz: string;
  format: TimeFormat;
  dateFormat: DateFormat;
}

/**
 * A log's moment to the millisecond, which is what orders two events of one
 * second: the time of day alone, with the day of the year before it, or in
 * full.
 */
export function formatLogMoment(
  micros: number,
  clock: LogClock,
  scope: 'time' | 'day' | 'full',
): string {
  const moment = zonedMoment(micros / 1_000, clock.tz);
  const time = moment.format(clock.format === 'local_12h' ? 'hh:mm:ss.SSS A' : 'HH:mm:ss.SSS');
  if (scope === 'time') return time;

  const day = DATE_PATTERNS[clock.dateFormat];

  // Within a page of results the year does not change the reading.
  return `${moment.format(scope === 'full' ? day : day.replace(/[-/]?YYYY[-/]?/, ''))} ${time}`;
}

/** Whether every moment falls on one day of the reader's clock, so the day need not be said on each. */
export function momentsShareDay(moments: readonly (number | null)[], tz: string): boolean {
  let earliest = Number.POSITIVE_INFINITY;
  let latest = Number.NEGATIVE_INFINITY;

  for (const micros of moments) {
    if (micros == null) continue;

    earliest = Math.min(earliest, micros);
    latest = Math.max(latest, micros);
  }
  // None of them has a moment: there is no day to tell apart.
  if (earliest > latest) return true;

  // The first and the last on one day leave no other day for those between them.
  const dayOf = (micros: number) => zonedMoment(micros / 1_000, tz).format('YYYY-MM-DD');

  return dayOf(earliest) === dayOf(latest);
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

/**
 * What a reader of logs looks for first, in the order the columns stand in:
 * how grave, from where, what was said, and then where the log belongs and
 * where it was written. Each is a meaning, with the names streams give it,
 * compared as plain names. The columns themselves are the stream's own fields:
 * none is assumed to be there.
 */
const COLUMN_MEANINGS: readonly (readonly string[])[] = [
  ['level', 'severity', 'severitytext', 'loglevel', 'lvl'],
  ['service', 'servicename', 'app', 'appname', 'application', 'component', 'source'],
  ['message', 'msg', 'body', 'log', 'content', 'summary'],
  ['traceid'],
  ['class', 'classname', 'logger', 'loggername', 'category'],
  ['caller', 'function', 'func', 'file', 'filename'],
  ['requestid', 'reqid'],
  ['host', 'hostname', 'node', 'instance'],
  ['pod', 'podname', 'container', 'containername'],
  ['thread', 'threadname'],
];

/** The columns a stream opens with beside the time: with it, five fields. */
export const DEFAULT_LOG_TABLE_FIELDS = 4;

/**
 * What does not make a column of its own accord: a flag says yes or no on
 * every row, a structure does not fit a cell, and the time has its column.
 */
const NOT_A_DEFAULT_COLUMN = new Set(['boolean', 'object', 'array', 'timestamp']);

export interface LogFieldPosition {
  index: number;
  previous?: string;
  next?: string;
}

export function captureLogFieldPosition(
  fields: readonly string[],
  field: string,
): LogFieldPosition | undefined {
  const index = fields.indexOf(field);
  if (index < 0) return undefined;

  return {
    index,
    ...(index > 0 ? { previous: fields[index - 1] } : {}),
    ...(index < fields.length - 1 ? { next: fields[index + 1] } : {}),
  };
}

export function restoreLogFieldPosition(
  fields: readonly string[],
  field: string,
  position?: LogFieldPosition,
): string[] {
  if (fields.includes(field)) return [...fields];

  const nextIndex = position?.next ? fields.indexOf(position.next) : -1;
  if (nextIndex >= 0) {
    return [...fields.slice(0, nextIndex), field, ...fields.slice(nextIndex)];
  }

  const previousIndex = position?.previous ? fields.indexOf(position.previous) : -1;
  if (previousIndex >= 0) {
    const insertAt = previousIndex + 1;
    return [...fields.slice(0, insertAt), field, ...fields.slice(insertAt)];
  }

  const insertAt = Math.min(Math.max(position?.index ?? fields.length, 0), fields.length);
  return [...fields.slice(0, insertAt), field, ...fields.slice(insertAt)];
}

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

/** The fields a record says its message in, in the order they are looked for. */
const STATEMENT_FIELDS = ['message', 'body', 'summary', 'log', 'event', 'error'] as const;

/**
 * What the record says: the text of its message, and the field it is in. A
 * record with no message of its own has none; the first field that happens to
 * hold something is not passed off as one.
 */
export function logStatement(record: Record<string, unknown>): { field: string; value: string } | null {
  for (const field of STATEMENT_FIELDS) {
    const value = readRecordValue(record, field);
    if (typeof value === 'string' && value.trim()) return { field, value };
  }

  return null;
}

/** The field that says where the log came from, and what it says. */
export function logSource(record: Record<string, unknown>): { field: string; value: unknown } | null {
  return firstPresentValue(record, SOURCE_FIELD_PRIORITY);
}

export function logSourceLabel(record: Record<string, unknown>): string {
  const source = logSource(record);
  return source ? displayLogValue(source.value) : '—';
}

export function logLevelLabel(record: Record<string, unknown>, fallback = 'INFO'): string {
  const level = firstPresentValue(record, LEVEL_FIELD_PRIORITY);
  return (level ? displayLogValue(level.value) : fallback).toUpperCase();
}

/** A field of a stream, as far as choosing its columns needs to know it. */
export interface LogColumnCandidate {
  name: string;
  /** The kind of value it holds, where that is known. */
  type?: string | undefined;
}

/** Letters and digits alone: `service.name`, `service_name` and `serviceName` are one name. */
function plainFieldName(name: string): string {
  return name.toLowerCase().replace(/[^a-z0-9]/g, '');
}

/** The names the end of a dotted field can be known by: `log.level` is a `level`, `k8s.pod.name` a `pod.name`. */
function endingsOf(name: string): string[] {
  const parts = name.split('.');

  return parts.slice(1).map((_, index) => plainFieldName(parts.slice(index + 1).join('.')));
}

/** The stream's field for a meaning: one named for it outright, or else one whose name ends in it. */
function fieldMeaning(meaning: readonly string[], fields: readonly LogColumnCandidate[]): string | null {
  for (const known of meaning) {
    const outright = fields.find((field) => plainFieldName(field.name) === known);
    if (outright) return outright.name;
  }

  for (const known of meaning) {
    const ending = fields.find((field) => endingsOf(field.name).includes(known));
    if (ending) return ending.name;
  }

  return null;
}

/**
 * The columns a stream opens with, beside the time: the fields it has for what
 * a reader looks for first, in that order, as many as `limit`. Where a stream
 * has no field for one of them the next is tried, and a stream that has few of
 * them is filled up with the fields it does have, in the order it has them.
 * Nothing is listed that the stream does not write.
 */
export function defaultLogTableFields(
  fields: readonly LogColumnCandidate[],
  limit = DEFAULT_LOG_TABLE_FIELDS,
): string[] {
  const available = fields.filter((field) => !TIMESTAMP_FIELDS.has(field.name));
  const chosen: string[] = [];

  for (const meaning of COLUMN_MEANINGS) {
    if (chosen.length >= limit) break;

    const name = fieldMeaning(meaning, available.filter((field) => !chosen.includes(field.name)));
    if (name) chosen.push(name);
  }

  for (const field of available) {
    if (chosen.length >= limit) break;
    if (chosen.includes(field.name) || field.name.startsWith('_')) continue;
    if (NOT_A_DEFAULT_COLUMN.has(field.type ?? '')) continue;

    chosen.push(field.name);
  }

  return chosen;
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

/** How high a row of the log list is: logs are read many at a time, a line each. */
export const LOG_RESULT_ROW_HEIGHT = 32;

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
