import type { StreamField, FieldType } from '@/api/streams';
import {
  andFieldClause,
  type FieldExpression,
  parseFieldExpression,
  quotedText,
} from '@/shell/query/fieldExpression';
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
  /** The stream keeps an index on the field: a filter on it does not read every row. */
  indexed?: boolean;
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

/** What one part of a field query says: a filter, text to find, or nothing the query can read. */
export type LogFieldPart =
  | { kind: 'filter'; filter: ParsedLogFieldClause }
  | { kind: 'text'; value: string }
  | { kind: 'rejected' };

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

/**
 * Adds a clause on a value. It narrows the whole statement: one that joins
 * alternatives with OR is put in brackets first.
 */
export function appendLogFieldValueClause(
  current: string,
  field: string,
  value: unknown,
  mode: 'include' | 'exclude' = 'include',
): string {
  const clause = `${field} ${mode === 'exclude' ? '!=' : '='} ${formatLogFieldQueryValue(value)}`;
  const trimmed = current.trim();
  if (trimmed.includes(clause)) return trimmed;
  return andFieldClause(trimmed, clause);
}

/** Adds a clause that asks for the records whose field has `text` somewhere in it. */
export function appendLogFieldContainsClause(current: string, field: string, text: string): string {
  const clause = `${field} contains ${formatLogFieldQueryValue(text)}`;
  const trimmed = current.trim();
  if (trimmed.includes(clause)) return trimmed;
  return andFieldClause(trimmed, clause);
}

export function appendLogFieldClause(current: string, field: LogFieldDef): string {
  const trimmed = current.trim();
  if (hasLogFieldClause(trimmed, field.name)) return current;
  return andFieldClause(trimmed, `${field.name} ${fieldPlaceholder(field.dataType)}`);
}

export function isLogFieldFilterable(field: LogFieldDef, mode: 'fields' | 'sql'): boolean {
  return mode === 'sql' || field.dataType !== 'json';
}

export function deriveLogFields(
  schemaFields: StreamField[],
  result?: QueryResult,
): LogFieldDef[] {
  if (schemaFields.length === 0 && !result) return [];
  const schema = new Map(schemaFields.map((field) => [field.name, field.dataType]));
  schema.set('_timestamp', 'timestamp');
  const indexed = new Set(schemaFields.filter((field) => field.indexed).map((field) => field.name));
  // Time is not a field a stream declares an index for: every stream is partitioned by it, and a
  // query reads only the files of its time range. It is the first index a log has.
  indexed.add('_timestamp');
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
      ...(indexed.has(name) ? { indexed: true } : {}),
    };
  });
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

/**
 * Whether a clause asks about the field having a value at all. A value written
 * as `''` is no value: `field = ''` is the records without one, `field != ''`
 * the records with one. It is what a field's "+" writes before a value is
 * typed, so it has to run.
 */
export function isLogFieldPresenceClause(
  clause: Pick<ParsedLogFieldClause, 'op' | 'value' | 'quoted'>,
): boolean {
  return clause.quoted && clause.value === '' && (clause.op === '=' || clause.op === '!=');
}

/**
 * Reads one part of a field query: a search function, a comparison, or text
 * to find, a quoted phrase as the words it holds. A part that opens a bracket
 * it does not close as a group is nothing the query can read.
 */
export function readLogFieldPart(part: string): LogFieldPart {
  // Functions are converted to structured filters before crossing the API
  // boundary. Raw function text must never be interpolated into backend SQL.
  const functionMatch = part.match(
    /^(MATCH_TEXT|MATCH)\s*\(\s*([a-zA-Z_][\w.]*)\s*,\s*([\s\S]*?)\s*\)$/i,
  );
  if (functionMatch) {
    const { value, quoted } = unquoteLogFieldValue(functionMatch[3]!);
    if (!quoted) return { kind: 'rejected' };
    return {
      kind: 'filter',
      filter: {
        field: functionMatch[2]!,
        op: functionMatch[1]!.toLowerCase() as 'match' | 'match_text',
        value,
        quoted: true,
      },
    };
  }
  if (/^(MATCH_TEXT|MATCH)\b/i.test(part) || part.startsWith('(')) return { kind: 'rejected' };

  const match = part.match(
    /^([a-zA-Z_][\w.]*)\s*(>=|<=|!=|=|>|<|eq\b|ne\b|contains\b|like\b)\s*([\s\S]+)$/i,
  );
  if (!match) {
    const phrase = quotedText(part);
    if (phrase === null) return { kind: 'text', value: part };
    return phrase.trim() ? { kind: 'text', value: phrase } : { kind: 'rejected' };
  }
  const rawOp = match[2]!.toLowerCase();
  const { value, quoted } = unquoteLogFieldValue(match[3]!);
  const op: ParsedLogFieldClause['op'] = rawOp === 'eq'
    ? '='
    : rawOp === 'ne'
      ? '!='
      : rawOp === 'like'
        ? 'contains'
        : rawOp as ParsedLogFieldClause['op'];
  if (!value && !isLogFieldPresenceClause({ op, value, quoted })) return { kind: 'rejected' };
  return { kind: 'filter', filter: { field: match[1]!, op, value, quoted } };
}

/** The parts of a field query that all hold: those of an AND outside brackets, or the one statement. */
function heldParts(statement: string): FieldExpression[] {
  const read = parseFieldExpression(statement);
  if (read === null) return [];
  return read.kind === 'and' ? read.items : [read];
}

/** The fields a field query compares anywhere, its alternatives included. */
export function logStatementFields(statement: string): string[] {
  const named = (expression: FieldExpression): string[] => {
    if (expression.kind !== 'part') return expression.items.flatMap(named);
    const read = readLogFieldPart(expression.text);
    return read.kind === 'filter' ? [read.filter.field] : [];
  };
  const read = parseFieldExpression(statement);
  return read === null ? [] : named(read);
}

/**
 * A field query read as the lists of what all has to hold: filters and text
 * to find. Alternatives, which a list cannot say, are kept with what it
 * cannot read, as written.
 */
export function parseLogFieldStatement(input: string): ParsedLogFieldStatement {
  const filters: ParsedLogFieldClause[] = [];
  const freeText: string[] = [];
  const rejected: string[] = [];

  for (const part of heldParts(input)) {
    const read = part.kind === 'part' ? readLogFieldPart(part.text) : { kind: 'rejected' as const };
    if (read.kind === 'filter') filters.push(read.filter);
    else if (read.kind === 'text') freeText.push(read.value);
    else rejected.push(part.text);
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
  // Cast to text so the comparison holds for a field of any type: a number is never ''.
  if (isLogFieldPresenceClause(clause)) {
    return clause.op === '='
      ? `(${field} IS NULL OR CAST(${field} AS VARCHAR) = '')`
      : `CAST(${field} AS VARCHAR) != ''`;
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

/** A part of a field query as SQL; `null` where it holds something the query cannot read. */
function logExpressionSql(expression: FieldExpression): string | null {
  if (expression.kind === 'part') {
    const read = readLogFieldPart(expression.text);
    if (read.kind === 'filter') return logFieldClauseToSql(read.filter);
    if (read.kind === 'text') return `"message" LIKE '%${escapeSqlString(read.value)}%'`;
    return null;
  }

  const items = expression.items.map(logExpressionSql);
  if (items.some((item) => item === null)) return null;
  return `(${items.join(expression.kind === 'and' ? ' AND ' : ' OR ')})`;
}

/**
 * The conditions a field statement selects by, each as SQL: one for each part
 * that has to hold, alternatives in brackets. A part the query cannot read
 * selects nothing here; the list refuses it.
 */
export function logFieldQueryConditions(statement: string): string[] {
  return heldParts(statement)
    .map(logExpressionSql)
    .filter((condition): condition is string => condition !== null);
}

export function buildLogFieldQuerySql(
  stream: string,
  statement: string,
  limit: number,
): string {
  const conditions = logFieldQueryConditions(statement);
  const where = conditions.length > 0 ? `\nWHERE ${conditions.join('\n  AND ')}` : '';
  return `SELECT * FROM "${escapeSqlIdentifier(stream)}"${where}\nORDER BY _timestamp DESC\nLIMIT ${limit}`;
}
