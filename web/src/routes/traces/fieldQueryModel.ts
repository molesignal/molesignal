import type { StreamField, FieldType, StreamSummary } from '@/api/streams';
import type { TraceFilter } from '@/api/web';

export type TraceFieldName = string;
export type TraceQueryMode = 'fields' | 'sql';
export type TraceFieldScope = 'span' | 'trace_aggregate';

export interface TraceFieldDef {
  name: TraceFieldName;
  dataType: FieldType;
  physical: boolean;
  scope: TraceFieldScope;
}

export interface TraceFieldGroup {
  group: string;
  fields: TraceFieldDef[];
}

export interface ParsedTraceStatement {
  q?: string;
  filters: TraceFilter[];
  rejected: string[];
}

export const TRACE_RESULT_LIMIT = 200;

const INTERNAL_TRACE_SUMMARY_PREFIX = 'molesignal.trace.';
const SYNTHETIC_TRACE_FIELDS: TraceFieldDef[] = [
  { name: 'span_count', dataType: 'int64', physical: false, scope: 'trace_aggregate' },
  { name: 'error_count', dataType: 'int64', physical: false, scope: 'trace_aggregate' },
];
const FALLBACK_TRACE_FIELDS: TraceFieldDef[] = [
  { name: 'trace_id', dataType: 'utf8', physical: true, scope: 'span' },
  { name: 'span_id', dataType: 'utf8', physical: true, scope: 'span' },
  { name: 'parent_span_id', dataType: 'utf8', physical: true, scope: 'span' },
  { name: 'name', dataType: 'utf8', physical: true, scope: 'span' },
  { name: 'service.name', dataType: 'utf8', physical: true, scope: 'span' },
  { name: 'status_code', dataType: 'utf8', physical: true, scope: 'span' },
  { name: 'duration_ns', dataType: 'int64', physical: true, scope: 'trace_aggregate' },
  ...SYNTHETIC_TRACE_FIELDS,
];
const CORE_FIELD_ORDER = [
  'trace_id',
  'span_id',
  'parent_span_id',
  'name',
  'status_code',
  'duration_ns',
  'span_count',
  'error_count',
];
const TRACE_FIELD_ALIASES: Record<string, string> = {
  service: 'service.name',
  service_name: 'service.name',
  operation: 'name',
  operation_name: 'name',
  status: 'status_code',
};
const REQUIRED_TRACE_FIELDS = new Set([
  'trace_id',
  'span_id',
  'service.name',
  'name',
  'start_time_unix_nano',
  'end_time_unix_nano',
  'status_code',
]);

export const DEFAULT_VISIBLE_TRACE_FIELDS: TraceFieldName[] = [
  'service.name',
  'duration_ns',
  'span_count',
  'status_code',
];
export const COMMON_TRACE_FIELD_ORDER = [
  'trace_id',
  'name',
  'service.name',
  'status_code',
  'duration_ns',
] as const;
export const COMMON_TRACE_FIELDS = new Set<string>(COMMON_TRACE_FIELD_ORDER);

export function selectTraceStream(streams: StreamSummary[]): StreamSummary | undefined {
  const rank = (stream: StreamSummary) => {
    const names = new Set(stream.schema.fields.map((field) => field.name));
    const canonical = [...REQUIRED_TRACE_FIELDS].every((field) => names.has(field));
    if (canonical && stream.name === 'default') return 3;
    if (canonical) return 2;
    return stream.name === 'default' ? 1 : 0;
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
      dataType: field.data_type,
      physical: true,
      scope: field.name === 'duration_ns' ? 'trace_aggregate' : 'span',
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
  const byPrefix = new Map<string, TraceFieldDef[]>();
  for (const field of fields) {
    const dot = field.name.indexOf('.');
    if (dot <= 0) {
      core.push(field);
      continue;
    }
    const prefix = field.name.slice(0, dot);
    const list = byPrefix.get(prefix) ?? [];
    list.push(field);
    byPrefix.set(prefix, list);
  }
  core.sort((left, right) => {
    const leftIndex = CORE_FIELD_ORDER.indexOf(left.name);
    const rightIndex = CORE_FIELD_ORDER.indexOf(right.name);
    if (leftIndex !== rightIndex) {
      return (leftIndex < 0 ? Infinity : leftIndex) - (rightIndex < 0 ? Infinity : rightIndex);
    }
    return left.name.localeCompare(right.name);
  });
  const groups = [...byPrefix.entries()]
    .sort(([left], [right]) => left.localeCompare(right))
    .map(([group, groupedFields]) => ({
      group,
      fields: [...groupedFields].sort((left, right) => left.name.localeCompare(right.name)),
    }));
  return { core, groups };
}

export function isTraceFieldQueryable(field: TraceFieldDef, mode: TraceQueryMode): boolean {
  if (mode === 'sql') return field.physical || field.scope === 'trace_aggregate';
  return field.dataType !== 'json';
}

export function parseTraceStatement(
  input: string,
  fields: TraceFieldDef[],
): ParsedTraceStatement {
  const trimmed = input.trim();
  if (!trimmed) return { filters: [], rejected: [] };

  const filters: TraceFilter[] = [];
  const rejected: string[] = [];
  const freeText: string[] = [];
  for (const rawPart of splitStatement(trimmed)) {
    const part = rawPart.trim();
    if (!part) continue;
    const match = part.match(
      /^([a-zA-Z_][\w.]*)\s*(>=|<=|!=|=|>|<|eq\b|ne\b|contains\b|like\b)\s*([\s\S]+)$/i,
    );
    if (!match) {
      freeText.push(part);
      continue;
    }
    const requestedField = match[1]!;
    const canonicalName = TRACE_FIELD_ALIASES[requestedField] ?? requestedField;
    const field = fields.find((candidate) => candidate.name === canonicalName);
    const value = unquoteTraceValue(match[3]!);
    const op = normalizeOperator(match[2]!);
    if (!field || !value || !operatorSupported(field.dataType, op) || field.dataType === 'json') {
      rejected.push(part);
      continue;
    }
    filters.push({ field: canonicalName, op, value });
  }

  return {
    ...(freeText.length > 0 ? { q: freeText.join(' ') } : {}),
    filters,
    rejected,
  };
}

export function insertTraceClause(current: string, field: TraceFieldDef): string {
  const trimmed = current.trim();
  if (hasTraceClause(trimmed, field.name)) return current;
  const clause = `${field.name} ${placeholderPredicate(field.dataType)}`;
  if (!trimmed) return clause;
  if (/\bAND\s*$/i.test(trimmed)) return `${trimmed} ${clause}`;
  return `${trimmed} AND ${clause}`;
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
  const aggregate = aggregateExpression(field.name);
  if (aggregate) return insertSqlClause(base, 'HAVING', `${aggregate} >= 0`);

  const fieldPredicate = sqlFieldPredicate(field);
  if (/\bGROUP\s+BY\b/i.test(base)) {
    return insertSqlClause(
      base,
      'HAVING',
      `MAX(CASE WHEN ${fieldPredicate} THEN 1 ELSE 0 END) = 1`,
    );
  }
  return insertSqlClause(base, 'WHERE', fieldPredicate);
}

function aggregateExpression(field: string): string | null {
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

function insertSqlClause(base: string, keyword: 'WHERE' | 'HAVING', clause: string): string {
  const boundaryPattern = keyword === 'WHERE'
    ? /\s+(GROUP\s+BY|HAVING|ORDER\s+BY|LIMIT)\b/i
    : /\s+(ORDER\s+BY|LIMIT)\b/i;
  const boundary = base.search(boundaryPattern);
  const before = boundary >= 0 ? base.slice(0, boundary).trimEnd() : base;
  const after = boundary >= 0 ? base.slice(boundary) : '';
  const hasClause = keyword === 'WHERE' ? /\bWHERE\b/i.test(before) : /\bHAVING\b/i.test(before);
  return `${before}${hasClause ? `\n  AND ${clause}` : `\n${keyword} ${clause}`}${after}`;
}

function hasTraceSqlFieldFilter(statement: string, field: string): boolean {
  const aggregate = aggregateExpression(field);
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
    `(?:^|\\bAND\\s+)${escapeRegExp(field)}\\s*(?:>=|<=|!=|=|>|<|eq\\b|ne\\b|contains\\b|like\\b)`,
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
  if (dataType === 'utf8') return ['=', '!=', 'contains'].includes(op);
  if (dataType === 'bool') return op === '=' || op === '!=';
  if (dataType === 'json') return false;
  return ['=', '!=', '>', '>=', '<', '<='].includes(op);
}

function splitStatement(input: string): string[] {
  const parts: string[] = [];
  let start = 0;
  let quote: "'" | '"' | null = null;
  let escaped = false;
  for (let index = 0; index < input.length; index += 1) {
    const character = input[index]!;
    if (escaped) {
      escaped = false;
      continue;
    }
    if (character === '\\') {
      escaped = true;
      continue;
    }
    if (quote) {
      if (character === quote) quote = null;
      continue;
    }
    if (character === "'" || character === '"') {
      quote = character;
      continue;
    }
    const conjunction = input.slice(index).match(/^\s+AND\s+/i);
    if (!conjunction) continue;
    parts.push(input.slice(start, index));
    index += conjunction[0].length - 1;
    start = index + 1;
  }
  parts.push(input.slice(start));
  return parts;
}

function unquoteTraceValue(value: string): string {
  const trimmed = value.trim();
  if (trimmed.length >= 2) {
    const first = trimmed[0];
    const last = trimmed[trimmed.length - 1];
    if ((first === "'" && last === "'") || (first === '"' && last === '"')) {
      return trimmed.slice(1, -1);
    }
  }
  return trimmed;
}

function escapeTraceSqlIdentifier(identifier: string): string {
  return identifier.replace(/"/g, '""');
}

function escapeRegExp(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}
