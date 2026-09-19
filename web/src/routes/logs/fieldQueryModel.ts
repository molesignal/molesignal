import type { StreamField, FieldType } from '@/api/streams';
import type { QueryResult } from '@/types/query';

export type LogFieldDisplayType =
  | 'string'
  | 'number'
  | 'timestamp'
  | 'date'
  | 'array'
  | 'boolean'
  | 'object';

export interface LogFieldDef {
  name: string;
  type: LogFieldDisplayType;
  dataType: FieldType;
  count: string;
  expandable: boolean;
}

export interface ParsedLogFieldClause {
  field: string;
  op: '=' | '!=' | '>' | '>=' | '<' | '<=' | 'contains' | 'match' | 'match_text';
  value: string;
  quoted: boolean;
}

export interface ParsedLogFieldStatement {
  filters: ParsedLogFieldClause[];
  freeText: string[];
  rejected: string[];
}

export function escapeSqlIdentifier(identifier: string): string {
  return identifier.replace(/"/g, '""');
}

export function escapeSqlString(value: string): string {
  return value.replace(/'/g, "''");
}

export function formatLogFieldQueryValue(value: unknown): string {
  if (typeof value === 'number' && Number.isFinite(value)) return String(value);
  if (typeof value === 'boolean') return String(value);
  const rendered = typeof value === 'string' ? value : JSON.stringify(value) ?? String(value);
  return `'${rendered
    .replace(/\\/g, '\\\\')
    .replace(/'/g, "\\'")
    .replace(/\n/g, '\\n')
    .replace(/\r/g, '\\r')
    .replace(/\t/g, '\\t')}'`;
}

export function appendLogFieldValueClause(
  current: string,
  field: string,
  value: unknown,
  mode: 'include' | 'exclude' = 'include',
): string {
  const clause = `${field} ${mode === 'exclude' ? '!=' : '='} ${formatLogFieldQueryValue(value)}`;
  const trimmed = current.trim();
  if (!trimmed) return clause;
  if (trimmed.includes(clause)) return trimmed;
  if (/\bAND\s*$/i.test(trimmed)) return `${trimmed} ${clause}`;
  return `${trimmed} AND ${clause}`;
}

export function appendLogFieldClause(current: string, field: LogFieldDef): string {
  const trimmed = current.trim();
  if (hasLogFieldClause(trimmed, field.name)) return current;
  const clause = `${field.name} ${fieldPlaceholder(field.dataType)}`;
  if (!trimmed) return clause;
  if (/\bAND\s*$/i.test(trimmed)) return `${trimmed} ${clause}`;
  return `${trimmed} AND ${clause}`;
}

export function isLogFieldFilterable(field: LogFieldDef, mode: 'fields' | 'sql'): boolean {
  return mode === 'sql' || field.dataType !== 'json';
}

export function deriveLogFields(
  schemaFields: StreamField[],
  result?: QueryResult,
): LogFieldDef[] {
  if (schemaFields.length === 0 && !result) return [];
  const schema = new Map(schemaFields.map((field) => [field.name, field.data_type]));
  schema.set('_timestamp', 'timestamp');
  const names = new Set<string>(schema.keys());
  result?.columns.forEach((name) => names.add(name));
  const resultIndex = new Map(result?.columns.map((name, index) => [name, index]) ?? []);

  return [...names].map((name) => {
    const columnIndex = resultIndex.get(name);
    const values = columnIndex === undefined || !result
      ? []
      : result.rows.map((row) => row[columnIndex]);
    const present = values.filter(isPresent).length;
    const dataType = schema.get(name) ?? inferFieldDataType(name, values);
    const type = displayType(name, dataType, values);
    return {
      name,
      type,
      dataType,
      count: result?.rows.length ? String(present) : '0',
      expandable: type === 'array' || type === 'object',
    };
  });
}

function splitLogFieldStatement(input: string): string[] {
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

    const remaining = input.slice(index);
    const conjunction = remaining.match(/^\s+AND\s+/i);
    if (!conjunction) continue;

    parts.push(input.slice(start, index));
    index += conjunction[0].length - 1;
    start = index + 1;
  }

  parts.push(input.slice(start));
  return parts;
}

function decodeLogFieldEscapes(value: string): string {
  let decoded = '';

  for (let index = 0; index < value.length; index += 1) {
    const character = value[index]!;
    if (character !== '\\' || index === value.length - 1) {
      decoded += character;
      continue;
    }

    const escaped = value[index + 1]!;
    const replacement = {
      '\\': '\\',
      "'": "'",
      '"': '"',
      n: '\n',
      r: '\r',
      t: '\t',
    }[escaped];

    if (replacement === undefined) {
      decoded += character;
      continue;
    }

    decoded += replacement;
    index += 1;
  }

  return decoded;
}

export function unquoteLogFieldValue(rawValue: string): { value: string; quoted: boolean } {
  const trimmed = rawValue.trim();
  if (trimmed.length >= 2) {
    const first = trimmed[0];
    const last = trimmed[trimmed.length - 1];
    if ((first === "'" && last === "'") || (first === '"' && last === '"')) {
      return {
        value: decodeLogFieldEscapes(trimmed.slice(1, -1)),
        quoted: true,
      };
    }
  }
  return { value: trimmed, quoted: false };
}

export function parseLogFieldStatement(input: string): ParsedLogFieldStatement {
  const trimmed = input.trim();
  if (!trimmed) return { filters: [], freeText: [], rejected: [] };

  const filters: ParsedLogFieldClause[] = [];
  const freeText: string[] = [];
  const rejected: string[] = [];

  for (const rawPart of splitLogFieldStatement(trimmed)) {
    const part = rawPart.trim();
    if (!part) continue;
    // Functions are converted to structured filters before crossing the API
    // boundary. Raw function text must never be interpolated into backend SQL.
    const functionMatch = part.match(
      /^(MATCH_TEXT|MATCH)\s*\(\s*([a-zA-Z_][\w.]*)\s*,\s*([\s\S]*?)\s*\)$/i,
    );
    if (functionMatch) {
      const { value, quoted } = unquoteLogFieldValue(functionMatch[3]!);
      if (!quoted) {
        rejected.push(part);
        continue;
      }
      filters.push({
        field: functionMatch[2]!,
        op: functionMatch[1]!.toLowerCase() as 'match' | 'match_text',
        value,
        quoted: true,
      });
      continue;
    }
    if (/^(MATCH_TEXT|MATCH)\b/i.test(part)) {
      rejected.push(part);
      continue;
    }
    const match = part.match(
      /^([a-zA-Z_][\w.]*)\s*(>=|<=|!=|=|>|<|eq\b|ne\b|contains\b|like\b)\s*([\s\S]+)$/i,
    );
    if (!match) {
      freeText.push(part);
      continue;
    }
    const rawOp = match[2]!.toLowerCase();
    const { value, quoted } = unquoteLogFieldValue(match[3]!);
    if (!value) {
      rejected.push(part);
      continue;
    }
    const op: ParsedLogFieldClause['op'] = rawOp === 'eq'
      ? '='
      : rawOp === 'ne'
        ? '!='
        : rawOp === 'like'
          ? 'contains'
          : rawOp as ParsedLogFieldClause['op'];
    filters.push({ field: match[1]!, op, value, quoted });
  }

  return { filters, freeText, rejected };
}

function sqlValueFromLogClause(clause: ParsedLogFieldClause): string {
  if (!clause.quoted && /^-?\d+(?:\.\d+)?$/.test(clause.value)) return clause.value;
  if (!clause.quoted && /^(true|false)$/i.test(clause.value)) return clause.value.toUpperCase();
  return `'${escapeSqlString(clause.value)}'`;
}

function normalizedWhitespaceExpression(field: string): string {
  return `regexp_replace(CAST(${field} AS VARCHAR), '[[:space:]]+', ' ', 'g')`;
}

function normalizedWhitespaceValue(value: string): string {
  return value.replace(/\s+/g, ' ');
}

export function logFieldClauseToSql(clause: ParsedLogFieldClause): string {
  const field = `"${escapeSqlIdentifier(clause.field)}"`;
  if (clause.op === 'match' || clause.op === 'match_text') {
    if (clause.op === 'match' && clause.value.length === 0) return 'FALSE';
    const functionName = clause.op === 'match' ? 'MATCH' : 'MATCH_TEXT';
    const functionField = /^[a-zA-Z_][\w]*$/.test(clause.field)
      ? clause.field
      : field;
    return `${functionName}(${functionField}, '${escapeSqlString(clause.value)}')`;
  }
  const normalizeRenderedWhitespace = clause.quoted && /\s/.test(clause.value);

  if (clause.op === 'contains') {
    const direct = `CAST(${field} AS VARCHAR) LIKE '%${escapeSqlString(clause.value)}%'`;
    if (!normalizeRenderedWhitespace) return direct;

    const normalized = normalizedWhitespaceValue(clause.value);
    return `(${direct} OR ${normalizedWhitespaceExpression(field)} LIKE '%${escapeSqlString(normalized)}%')`;
  }

  const direct = `${field} ${clause.op} ${sqlValueFromLogClause(clause)}`;
  if (!normalizeRenderedWhitespace) return direct;

  // HTML collapses line breaks and repeated whitespace when a log value is
  // shown inline. Keep "=" useful for the text the user actually sees while
  // retaining the raw exact comparison as the fast path.
  const normalized = `'${escapeSqlString(normalizedWhitespaceValue(clause.value))}'`;
  const normalizedComparison = `${normalizedWhitespaceExpression(field)} ${clause.op} ${normalized}`;
  return clause.op === '='
    ? `(${direct} OR ${normalizedComparison})`
    : `(${direct} AND ${normalizedComparison})`;
}

function hasLogFieldClause(current: string, field: string): boolean {
  const pattern = new RegExp(
    `(?:^|\\bAND\\s+)${escapeRegExp(field)}\\s*(?:>=|<=|!=|=|>|<|eq\\b|ne\\b|contains\\b|like\\b)`,
    'i',
  );
  return pattern.test(current.trim());
}

function fieldPlaceholder(dataType: FieldType): string {
  switch (dataType) {
    case 'bool':
      return '= true';
    case 'int64':
    case 'float64':
    case 'timestamp':
      return '>= 0';
    case 'json':
      return "contains 'key'";
    default:
      return "= ''";
  }
}

function displayType(
  name: string,
  dataType: FieldType,
  values: unknown[],
): LogFieldDisplayType {
  const sample = values.find(isPresent);
  if (Array.isArray(sample)) return 'array';
  if (sample !== null && typeof sample === 'object') return 'object';
  if (dataType === 'json') return 'object';
  if (dataType === 'bool') return 'boolean';
  if (dataType === 'int64' || dataType === 'float64') return 'number';
  if (dataType === 'timestamp') return 'timestamp';
  const lower = name.toLowerCase();
  if (typeof sample === 'string') {
    const parsed = Date.parse(sample);
    if ((lower.endsWith('_at') || lower.endsWith('_date')) && Number.isFinite(parsed)) return 'date';
  }
  return 'string';
}

function inferFieldDataType(name: string, values: unknown[]): FieldType {
  const lower = name.toLowerCase();
  if (lower === '_timestamp' || lower.endsWith('_time') || lower.includes('timestamp')) {
    return 'timestamp';
  }
  const sample = values.find(isPresent);
  if (typeof sample === 'boolean') return 'bool';
  if (typeof sample === 'number') return Number.isInteger(sample) ? 'int64' : 'float64';
  if (Array.isArray(sample) || (sample !== null && typeof sample === 'object')) return 'json';
  return 'utf8';
}

function isPresent(value: unknown): boolean {
  return value !== null && value !== undefined && value !== '';
}

function escapeRegExp(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

export function buildLogFieldQuerySql(
  stream: string,
  statement: string,
  limit: number,
): string {
  const parsed = parseLogFieldStatement(statement);
  const conditions = parsed.filters.map(logFieldClauseToSql);
  parsed.freeText.forEach((part) => {
    conditions.push(`"message" LIKE '%${escapeSqlString(part)}%'`);
  });
  const where = conditions.length > 0 ? `\nWHERE ${conditions.join('\n  AND ')}` : '';
  return `SELECT * FROM "${escapeSqlIdentifier(stream)}"${where}\nORDER BY _timestamp DESC\nLIMIT ${limit}`;
}
