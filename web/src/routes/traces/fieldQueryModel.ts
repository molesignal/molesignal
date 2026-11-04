import type { StreamField, FieldType, StreamSummary } from '@/api/streams';
import type { TraceCondition, TraceFilter } from '@/api/web';
import {
  andFieldClause,
  type FieldExpression,
  fieldConjuncts,
  hasTopLevelOr,
  parseFieldExpression,
  quotedText,
} from '@/shell/query/fieldExpression';
import { addSqlCondition } from '@/shell/query/sqlCondition';

import { MAX_TRACE_IN_VALUES, traceInClauseOf, unquoteTraceValue } from './inClause';

export type TraceFieldName = string;
export type TraceQueryMode = 'fields' | 'sql';
export type TraceFieldScope = 'span' | 'trace_aggregate';

export interface TraceFieldDef {
  name: TraceFieldName;
  dataType: FieldType;
  physical: boolean;
  scope: TraceFieldScope;
  /** The stream keeps an index on the field: a filter on it does not read every row. */
  indexed?: boolean;
}

export interface TraceFieldGroup {
  group: TraceAttributeGroup;
  fields: TraceFieldDef[];
}

export type TraceAttributeGroup = 'resource' | 'scope' | 'span';

export interface ParsedTraceStatement {
  q?: string;
  filters: TraceFilter[];
  /** What the statement asks beyond the parts that all hold: its alternatives, as the trace reads take them. */
  where?: TraceCondition;
  rejected: string[];
}

/** What one part of a field query says: a comparison, text to find, or nothing the trace list can read. */
export type TracePart =
  | { kind: 'filter'; filter: TraceFilter }
  | { kind: 'text'; value: string }
  | { kind: 'rejected' };

export const TRACE_RESULT_LIMIT = 200;

const INTERNAL_TRACE_SUMMARY_PREFIX = 'molesignal.trace.';
const TRACE_SUMMARY_MARKER_FIELD = `${INTERNAL_TRACE_SUMMARY_PREFIX}summary`;
const SYNTHETIC_TRACE_FIELDS: TraceFieldDef[] = [
  { name: 'span_count', dataType: 'int64', physical: false, scope: 'trace_aggregate' },
  { name: 'error_count', dataType: 'int64', physical: false, scope: 'trace_aggregate' },
];
// What a trace stream holds before its schema is known, with the indexes a new stream is given.
const FALLBACK_TRACE_FIELDS: TraceFieldDef[] = [
  { name: 'trace_id', dataType: 'utf8', physical: true, scope: 'span', indexed: true },
  { name: 'span_id', dataType: 'utf8', physical: true, scope: 'span', indexed: true },
  { name: 'parent_span_id', dataType: 'utf8', physical: true, scope: 'span', indexed: true },
  { name: 'name', dataType: 'utf8', physical: true, scope: 'span' },
  { name: 'service.name', dataType: 'utf8', physical: true, scope: 'span', indexed: true },
  { name: 'status_code', dataType: 'utf8', physical: true, scope: 'span' },
  { name: 'duration_ns', dataType: 'int64', physical: true, scope: 'trace_aggregate' },
  ...SYNTHETIC_TRACE_FIELDS,
];
const CORE_FIELD_ORDER = [
  'trace_id',
  'span_id',
  'parent_span_id',
  'name',
  'kind',
  'span_kind',
  'start_time',
  'start_time_unix_nano',
  'end_time',
  'end_time_unix_nano',
  'duration_ns',
  'status_code',
  'status_message',
  'trace_state',
  'trace_flags',
  'flags',
  'span_count',
  'error_count',
  'events',
  'links',
  'dropped_attributes_count',
  'dropped_events_count',
  'dropped_links_count',
];
const CORE_TRACE_FIELDS = new Set<string>(CORE_FIELD_ORDER);
const TRACE_ATTRIBUTE_GROUP_ORDER: TraceAttributeGroup[] = ['resource', 'scope', 'span'];
const RESOURCE_ATTRIBUTE_NAMES = new Set([
  'resource',
  'resource_attributes',
  'resource_schema_url',
]);
const RESOURCE_ATTRIBUTE_PREFIXES = [
  'resource.',
  'service.',
  'deployment.',
  'cloud.',
  'container.',
  'host.',
  'k8s.',
  'os.',
  'process.',
  'telemetry.sdk.',
  'telemetry.distro.',
  'device.',
  'faas.',
  'browser.',
];
const SCOPE_ATTRIBUTE_NAMES = new Set([
  'instrumentation_scope',
  'scope_attributes',
  'scope_name',
  'scope_version',
  'scope_schema_url',
]);
const SCOPE_ATTRIBUTE_PREFIXES = [
  'instrumentation_scope.',
  'otel.scope.',
  'otel.library.',
  'scope.',
];
const REQUIRED_TRACE_FIELDS = new Set([
  'trace_id',
  'span_id',
  'service.name',
  'name',
  'start_time_unix_nano',
  'end_time_unix_nano',
  'status_code',
]);

// A field query names its fields bare, so it only reaches the ones whose name reads as one word.
const FIELD_QUERY_NAME = /^[a-zA-Z_][\w.]*$/;
const TRACE_LEVEL_FIELDS = new Set(['trace_id', 'duration_ns', 'span_count', 'error_count']);

/**
 * Whether a trace has one value for the field. The backend compares these four
 * on the trace itself. Every other field is of a span: a filter on it keeps a
 * trace when one of its spans has the value, and `!=` keeps a trace when none
 * of its spans has it.
 */
export function isTraceLevelField(name: TraceFieldName): boolean {
  return TRACE_LEVEL_FIELDS.has(name);
}

export function selectTraceStream(
  streams: StreamSummary[],
  preferredName?: string,
): StreamSummary | undefined {
  const requested = preferredName?.trim();
  if (requested) {
    const preferred = streams.find((stream) => stream.name === requested);
    if (preferred) return preferred;
  }
  const rank = (stream: StreamSummary) => {
    const names = new Set(stream.schema.fields.map((field) => field.name));
    const canonical = [...REQUIRED_TRACE_FIELDS].every((field) => names.has(field));
    // The same order as the backend's choice, which the trace list reads: the standard columns,
    // then summaries, then the name `default`.
    return (canonical ? 4 : 0) + (names.has(TRACE_SUMMARY_MARKER_FIELD) ? 2 : 0) + (stream.name === 'default' ? 1 : 0);
  };
  return [...streams].sort((left, right) => (
    rank(right) - rank(left) || left.name.localeCompare(right.name)
  ))[0];
}

export function deriveTraceFields(schemaFields: StreamField[]): TraceFieldDef[] {
  if (schemaFields.length === 0) return FALLBACK_TRACE_FIELDS.map((field) => ({ ...field }));
  const fields = new Map<string, TraceFieldDef>();
  for (const field of schemaFields) {
    if (!field.name || field.name === '_timestamp' || field.name.startsWith(INTERNAL_TRACE_SUMMARY_PREFIX)) {
      continue;
    }
    fields.set(field.name, {
      name: field.name,
      dataType: field.dataType,
      physical: true,
      scope: field.name === 'duration_ns' ? 'trace_aggregate' : 'span',
      ...(field.indexed ? { indexed: true } : {}),
    });
  }
  for (const field of SYNTHETIC_TRACE_FIELDS) fields.set(field.name, { ...field });
  return [...fields.values()];
}

export function groupTraceFields(fields: TraceFieldDef[]): {
  core: TraceFieldDef[];
  groups: TraceFieldGroup[];
} {
  const core: TraceFieldDef[] = [];
  const byGroup = new Map<TraceAttributeGroup, TraceFieldDef[]>(
    TRACE_ATTRIBUTE_GROUP_ORDER.map((group) => [group, []]),
  );
  for (const field of fields) {
    if (CORE_TRACE_FIELDS.has(field.name)) {
      core.push(field);
      continue;
    }
    byGroup.get(traceAttributeGroup(field.name))!.push(field);
  }
  core.sort((left, right) => {
    const leftIndex = CORE_FIELD_ORDER.indexOf(left.name);
    const rightIndex = CORE_FIELD_ORDER.indexOf(right.name);
    if (leftIndex !== rightIndex) {
      return (leftIndex < 0 ? Infinity : leftIndex) - (rightIndex < 0 ? Infinity : rightIndex);
    }
    return left.name.localeCompare(right.name);
  });
  const groups = TRACE_ATTRIBUTE_GROUP_ORDER
    .map((group) => ({
      group,
      fields: [...byGroup.get(group)!].sort((left, right) => left.name.localeCompare(right.name)),
    }))
    .filter((group) => group.fields.length > 0);
  return { core, groups };
}

function traceAttributeGroup(fieldName: string): TraceAttributeGroup {
  if (
    RESOURCE_ATTRIBUTE_NAMES.has(fieldName)
    || RESOURCE_ATTRIBUTE_PREFIXES.some((prefix) => fieldName.startsWith(prefix))
  ) {
    return 'resource';
  }
  if (
    SCOPE_ATTRIBUTE_NAMES.has(fieldName)
    || SCOPE_ATTRIBUTE_PREFIXES.some((prefix) => fieldName.startsWith(prefix))
  ) {
    return 'scope';
  }
  return 'span';
}

export function isTraceFieldQueryable(field: TraceFieldDef, mode: TraceQueryMode): boolean {
  if (mode === 'sql') return field.physical || field.scope === 'trace_aggregate';
  return field.dataType !== 'json' && FIELD_QUERY_NAME.test(field.name);
}

/**
 * One comparison of a field query, read from a part of the statement: a field
 * and a value, or a field and a list of values; `null` for anything else. A
 * field is named as the stream names it: no other name stands for it.
 */
export function traceClauseOf(part: string): TraceFilter | null {
  // Alternatives are no one comparison, though the first of them reads like one.
  if (hasTopLevelOr(part)) return null;

  const list = traceInClauseOf(part);
  if (list) return list;

  const match = part.trim().match(
    /^([a-zA-Z_][\w.]*)\s*(>=|<=|!=|=|>|<|eq\b|ne\b|contains\b|like\b)\s*([\s\S]*)$/i,
  );
  if (!match) return null;

  return {
    field: match[1]!,
    op: normalizeOperator(match[2]!),
    value: unquoteTraceValue(match[3]!),
  };
}

/**
 * Reads one part of a field query: a comparison on a field the stream has, in
 * a way its type takes, or text to find, a quoted phrase as the words it
 * holds. A part that opens a bracket it does not close as a group, or that the
 * stream cannot answer, is rejected.
 */
export function readTracePart(part: string, fields: TraceFieldDef[]): TracePart {
  if (part.startsWith('(')) return { kind: 'rejected' };

  const clause = traceClauseOf(part);
  if (!clause) {
    const phrase = quotedText(part);
    if (phrase === null) return { kind: 'text', value: part };
    return phrase.trim() ? { kind: 'text', value: phrase } : { kind: 'rejected' };
  }

  const field = fields.find((candidate) => candidate.name === clause.field);
  const listed = clause.values?.length ?? 0;
  const empty = clause.op === 'in' ? listed === 0 || listed > MAX_TRACE_IN_VALUES : !clause.value;
  if (!field || empty || !operatorSupported(field.dataType, clause.op) || field.dataType === 'json') {
    return { kind: 'rejected' };
  }

  return { kind: 'filter', filter: clause };
}

/**
 * A group of the statement as the trace reads take it; `null` when a part of
 * it is one the list cannot read. The text of one group is one phrase, as the
 * list's free text is.
 */
function traceConditionOf(expression: FieldExpression, fields: TraceFieldDef[]): TraceCondition | null {
  if (expression.kind === 'part') {
    const read = readTracePart(expression.text, fields);
    if (read.kind === 'rejected') return null;
    return read.kind === 'filter' ? read.filter : { text: read.value };
  }

  const items: TraceCondition[] = [];
  const texts: string[] = [];

  for (const item of expression.items) {
    const read = traceConditionOf(item, fields);
    if (read === null) return null;
    if (expression.kind === 'and' && 'text' in read) texts.push(read.text);
    else items.push(read);
  }

  if (texts.length > 0) items.push({ text: texts.join(' ') });
  if (items.length === 1) return items[0]!;
  return expression.kind === 'and' ? { and: items } : { or: items };
}

/**
 * A field query as the trace reads take it. What all holds is the list of
 * filters and the free text, one phrase; alternatives are a condition tree.
 * A part the list cannot read is left out and said, with the whole of the
 * alternatives it stands in.
 */
export function parseTraceStatement(
  input: string,
  fields: TraceFieldDef[],
): ParsedTraceStatement {
  const read = parseFieldExpression(input);
  const parts = read === null ? [] : read.kind === 'and' ? read.items : [read];
  const filters: TraceFilter[] = [];
  const freeText: string[] = [];
  const alternatives: TraceCondition[] = [];
  const rejected: string[] = [];

  for (const part of parts) {
    if (part.kind !== 'part') {
      const condition = traceConditionOf(part, fields);
      if (condition) alternatives.push(condition);
      else rejected.push(part.text);
      continue;
    }

    const leaf = readTracePart(part.text, fields);
    if (leaf.kind === 'filter') filters.push(leaf.filter);
    else if (leaf.kind === 'text') freeText.push(leaf.value);
    else rejected.push(part.text);
  }

  const where = alternatives.length > 1 ? { and: alternatives } : alternatives[0];

  return {
    ...(freeText.length > 0 ? { q: freeText.join(' ') } : {}),
    filters,
    ...(where ? { where } : {}),
    rejected,
  };
}

export function insertTraceClause(current: string, field: TraceFieldDef): string {
  const trimmed = current.trim();
  if (hasTraceClause(trimmed, field.name)) return current;
  return andFieldClause(trimmed, `${field.name} ${placeholderPredicate(field.dataType)}`);
}

export function traceSqlTemplate(streamName: string): string {
  const table = escapeTraceSqlIdentifier(streamName);
  return `SELECT trace_id,
  MIN("service.name") AS service,
  MIN("name") AS operation,
  MIN(start_time_unix_nano) AS start_ns,
  MAX(end_time_unix_nano) - MIN(start_time_unix_nano) AS duration_ns,
  (MAX(end_time_unix_nano) - MIN(start_time_unix_nano)) / 1000000.0 AS duration_ms,
  COUNT(*) AS span_count,
  SUM(CASE WHEN status_code = 'ERROR' THEN 1 ELSE 0 END) AS error_count
FROM "${table}"
GROUP BY trace_id
ORDER BY start_ns DESC
LIMIT ${TRACE_RESULT_LIMIT}`;
}

export function traceSqlPlaceholder(streamName?: string): string {
  const table = escapeTraceSqlIdentifier(streamName?.trim() || 'trace_stream');
  return `SELECT * FROM "${table}"
WHERE status_code = 'ERROR'
ORDER BY start_time_unix_nano DESC
LIMIT ${TRACE_RESULT_LIMIT}`;
}

export function appendTraceSqlFieldFilter(
  statement: string,
  field: TraceFieldDef,
  streamName: string,
): string {
  const base = statement.trim() || traceSqlTemplate(streamName);
  if (hasTraceSqlFieldFilter(base, field.name)) return statement;
  const grouped = isGroupedTraceSql(base);
  const aggregate = traceAggregateExpression(field.name);
  if (aggregate && grouped) return insertTraceSqlClause(base, 'HAVING', `${aggregate} >= 0`);
  // A statement that lists spans has no trace to count: the count is asked of the stream.
  if (aggregate && !field.physical) {
    return insertTraceSqlClause(base, 'WHERE', traceCountSubquery(streamName, `${aggregate} >= 0`));
  }

  const fieldPredicate = sqlFieldPredicate(field);
  if (grouped) {
    return insertTraceSqlClause(
      base,
      'HAVING',
      `MAX(CASE WHEN ${fieldPredicate} THEN 1 ELSE 0 END) = 1`,
    );
  }
  return insertTraceSqlClause(base, 'WHERE', fieldPredicate);
}

/** Whether the statement has a row per trace (it groups) or a row per span. */
export function isGroupedTraceSql(statement: string): boolean {
  return /\bGROUP\s+BY\b/i.test(statement);
}

/** The traces of a stream that a condition on a count holds for, as a condition on a span's trace. */
export function traceCountSubquery(streamName: string, condition: string): string {
  return `trace_id IN (SELECT trace_id FROM "${escapeTraceSqlIdentifier(streamName)}" GROUP BY trace_id HAVING ${condition})`;
}

/** What a field that is worked out per trace is in SQL; `null` for a column. */
export function traceAggregateExpression(field: string): string | null {
  switch (field) {
    case 'duration_ns':
      return '(MAX(end_time_unix_nano) - MIN(start_time_unix_nano))';
    case 'span_count':
      return 'COUNT(*)';
    case 'error_count':
      return "SUM(CASE WHEN status_code = 'ERROR' THEN 1 ELSE 0 END)";
    default:
      return null;
  }
}

function sqlFieldPredicate(field: TraceFieldDef): string {
  const identifier = `"${escapeTraceSqlIdentifier(field.name)}"`;
  if (field.dataType === 'json') {
    return `CAST(${identifier} AS VARCHAR) LIKE '%"key"%'`;
  }
  if (field.dataType === 'timestamp') {
    return `CAST(${identifier} AS BIGINT) >= 0`;
  }
  return `${identifier} ${placeholderPredicate(field.dataType, true)}`;
}

function placeholderPredicate(dataType: FieldType, sql = false): string {
  switch (dataType) {
    case 'bool':
      return `= ${sql ? 'TRUE' : 'true'}`;
    case 'int64':
    case 'float64':
    case 'timestamp':
      return '>= 0';
    case 'json':
      return `contains '${sql ? '"key"' : 'key'}'`;
    default:
      return "= ''";
  }
}

/**
 * The statement with one more condition in its WHERE or HAVING. A condition
 * there that joins alternatives with OR is put in brackets first.
 */
export function insertTraceSqlClause(base: string, keyword: 'WHERE' | 'HAVING', clause: string): string {
  return addSqlCondition(base, keyword, clause);
}

function hasTraceSqlFieldFilter(statement: string, field: string): boolean {
  const aggregate = traceAggregateExpression(field);
  if (aggregate && statement.toLowerCase().includes(aggregate.toLowerCase())) {
    const aliasPattern = new RegExp(`\\b${escapeRegExp(field)}\\b\\s*(?:>=|<=|!=|=|>|<)`, 'i');
    if (aliasPattern.test(statement)) return true;
  }
  const quotedField = `"${escapeTraceSqlIdentifier(field)}"`;
  const pattern = new RegExp(
    `(?:^|\\s|\\()(${escapeRegExp(quotedField)}|${escapeRegExp(field)})\\s*(?:>=|<=|!=|<>|=|>|<|LIKE\\b|IN\\s*\\(|IS\\s+(?:NOT\\s+)?NULL)`,
    'i',
  );
  return pattern.test(statement);
}

function hasTraceClause(current: string, field: string): boolean {
  const pattern = new RegExp(
    `(?:^|\\bAND\\s+)${escapeRegExp(field)}\\s*(?:>=|<=|!=|=|>|<|eq\\b|ne\\b|contains\\b|like\\b|in\\s*\\()`,
    'i',
  );
  return pattern.test(current.trim());
}

function normalizeOperator(raw: string): TraceFilter['op'] {
  const op = raw.toLowerCase();
  if (op === 'eq') return '=';
  if (op === 'ne') return '!=';
  if (op === 'like') return 'contains';
  return op as TraceFilter['op'];
}

function operatorSupported(dataType: FieldType, op: TraceFilter['op']): boolean {
  if (dataType === 'utf8') return ['=', '!=', 'contains', 'in'].includes(op);
  if (dataType === 'bool') return op === '=' || op === '!=' || op === 'in';
  if (dataType === 'json') return false;
  return ['=', '!=', '>', '>=', '<', '<=', 'in'].includes(op);
}

/**
 * The parts of a field query that all have to hold, as written: what an `AND`
 * outside quotes and brackets joins. A statement with an `OR` outside
 * brackets is one part.
 */
export function splitTraceStatement(input: string): string[] {
  return fieldConjuncts(input);
}

export function escapeTraceSqlIdentifier(identifier: string): string {
  return identifier.replace(/"/g, '""');
}

function escapeRegExp(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}
