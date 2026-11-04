import type { FieldType } from '@/api/streams';
import type { TraceFilter } from '@/api/web';
import { isBareValue, type BuilderFieldKind } from '@/shell/query/builder/model';
import { joinFieldConjuncts } from '@/shell/query/fieldExpression';

import {
  escapeTraceSqlIdentifier,
  insertTraceSqlClause,
  isGroupedTraceSql,
  isTraceFieldQueryable,
  isTraceLevelField,
  splitTraceStatement,
  traceAggregateExpression,
  traceClauseOf,
  traceCountSubquery,
  traceSqlTemplate,
  type TraceFieldDef,
  type TraceQueryMode,
} from './fieldQueryModel';
import { MAX_TRACE_IN_VALUES, writeTraceInClause } from './inClause';
import { quoteTraceValue } from './urlState';

/** What a value picked in the field list can be asked of: it, not it, at least it, at most it. */
export type TraceValueOperator = '=' | '!=' | '>=' | '<=';

// The longest value the trace list takes in a filter, in bytes.
const MAX_FILTER_VALUE_BYTES = 512;

const isNumber = (dataType: FieldType) => dataType === 'int64' || dataType === 'float64' || dataType === 'timestamp';

function fieldKind(dataType: FieldType): BuilderFieldKind {
  if (dataType === 'bool') return 'boolean';
  return isNumber(dataType) ? 'number' : 'text';
}

/** Whether the value is one the field's type can be compared with. */
function isValueOfType(dataType: FieldType, value: string): boolean {
  if (dataType === 'int64' || dataType === 'timestamp') return /^-?\d+$/.test(value);
  if (dataType === 'float64') return value.trim() !== '' && Number.isFinite(Number(value));
  if (dataType === 'bool') return /^(true|false)$/i.test(value);
  return dataType === 'utf8';
}

/**
 * Whether a filter on this value can be written in the language in use, and
 * mean what the button says.
 *
 * A field query keeps a trace when one of its spans has the value, or with
 * `!=` when none of them has it; SQL asks the same of a statement with a row
 * per trace. Both mean what their buttons say on every field.
 */
export function canFilterTraceValue(
  field: TraceFieldDef,
  value: string,
  op: TraceValueOperator,
  mode: TraceQueryMode,
): boolean {
  if (field.dataType === 'json' || !isTraceFieldQueryable(field, mode)) return false;
  if (!isValueOfType(field.dataType, value)) return false;
  if ((op === '>=' || op === '<=') && !isNumber(field.dataType)) return false;
  if (mode === 'sql') return value !== '';

  // The trace list trims a value and caps its length, so one that starts or ends
  // with a space, or runs long, would never match itself.
  return value !== ''
    && value === value.trim()
    && !value.includes('\0')
    && new TextEncoder().encode(value).length <= MAX_FILTER_VALUE_BYTES;
}

function fieldQueryLiteral(field: TraceFieldDef, value: string): string {
  return isBareValue(fieldKind(field.dataType), value) ? value.trim() : `'${quoteTraceValue(value)}'`;
}

const boundOf = (op: TraceFilter['op']): 'lower' | 'upper' | null => {
  if (op === '>' || op === '>=') return 'lower';
  if (op === '<' || op === '<=') return 'upper';
  return null;
};

/** Whether a comparison already in the statement gives way to the one being added. */
function gaveWay(existing: TraceFilter, field: TraceFieldDef, op: TraceValueOperator, value: string): boolean {
  if (existing.field !== field.name) return false;
  // What the field's own "+" leaves for a value to be typed into.
  if (existing.value === '') return true;

  const equality = (candidate: TraceFilter['op']) => candidate === '=' || candidate === '!=';
  if (equality(op) && equality(existing.op)) {
    // The same value asked the other way round; or a second value for a field that has one.
    return existing.value === value || (op === '=' && existing.op === '=' && isTraceLevelField(field.name));
  }

  return boundOf(op) !== null && boundOf(op) === boundOf(existing.op);
}

/**
 * The parts of a field query, and whether one of them is this comparison,
 * however it is written: a value asked for is also one a list of the field
 * holds.
 */
function fieldQueryParts(statement: string, field: TraceFieldDef, value: string, op: TraceValueOperator) {
  const parts = splitTraceStatement(statement.trim().replace(/\s+AND\s*$/i, ''))
    .map((part) => part.trim())
    .filter(Boolean);
  const same = (part: string) => {
    const existing = traceClauseOf(part);
    if (existing?.field !== field.name) return false;
    if (op === '=' && existing.op === 'in') return existing.values?.includes(value) ?? false;
    return existing.op === op && existing.value === value;
  };

  return { parts, same };
}

/** The values a part asks the field for, when it asks for one or a list of them: what another value joins. */
function askedValues(part: string, field: TraceFieldDef): string[] | null {
  const existing = traceClauseOf(part);
  if (existing?.field !== field.name) return null;
  if (existing.op === 'in') return existing.values ?? [];
  return existing.op === '=' && existing.value !== '' ? [existing.value] : null;
}

/** A field asked for these values: one is a comparison, more are a list. */
function askingFor(field: TraceFieldDef, values: readonly string[]): string {
  return values.length === 1
    ? `${field.name} = ${fieldQueryLiteral(field, values[0]!)}`
    : writeTraceInClause(field.name, values.map((value) => fieldQueryLiteral(field, value)));
}

/**
 * Adds a comparison on a value to a field query.
 *
 * The statement is kept as it was written. A value asked for where the field
 * is asked for others already joins them in a list. Otherwise only a
 * comparison the new one answers gives way to it: the empty one the field's
 * "+" left, the same value asked the other way round, an earlier bound on the
 * same side.
 */
export function insertTraceValueClause(
  current: string,
  field: TraceFieldDef,
  value: string,
  op: TraceValueOperator,
): string {
  const clause = `${field.name} ${op} ${fieldQueryLiteral(field, value)}`;
  const { parts, same } = fieldQueryParts(current, field, value, op);
  if (parts.some(same)) return current;

  // A second value asked for joins the first: the field is asked for any of them, as boxes ticked in a list.
  const listAt = op === '=' ? parts.findIndex((part) => askedValues(part, field) !== null) : -1;
  if (listAt >= 0) {
    const values = askedValues(parts[listAt]!, field)!;
    if (values.length >= MAX_TRACE_IN_VALUES) return current;

    const asked = parts
      .map((part, index) => (index === listAt ? askingFor(field, [...values, value]) : part))
      .filter((part, index) => {
        const existing = traceClauseOf(part);
        return index === listAt || existing === null || existing.op === 'in' || !gaveWay(existing, field, op, value);
      });

    return joinFieldConjuncts(asked);
  }

  // The new comparison takes the place of the first one it answers; the rest of those go.
  const next: string[] = [];
  let placed = false;

  for (const part of parts) {
    const existing = traceClauseOf(part);
    // A list of the field stays; a value in it asked the other way round leaves it.
    if (existing?.op === 'in' && existing.field === field.name) {
      const rest = (existing.values ?? []).filter((asked) => !(op === '!=' && asked === value));
      if (rest.length === existing.values?.length) next.push(part);
      else if (rest.length > 0) next.push(askingFor(field, rest));
      continue;
    }
    if (existing === null || !gaveWay(existing, field, op, value)) {
      next.push(part);
    } else if (!placed) {
      next.push(clause);
      placed = true;
    }
  }

  // A statement of alternatives is narrowed as a whole: it goes in brackets beside the clause.
  return joinFieldConjuncts(placed ? next : [...next, clause]);
}

function sqlLiteral(field: TraceFieldDef, value: string): string {
  if (field.dataType === 'bool') return value.toUpperCase();
  return isNumber(field.dataType) ? value.trim() : `'${value.replace(/'/g, "''")}'`;
}

function sqlColumn(field: TraceFieldDef): string {
  const identifier = `"${escapeTraceSqlIdentifier(field.name)}"`;
  // A timestamp compares as the number it is stored as.
  return field.dataType === 'timestamp' ? `CAST(${identifier} AS BIGINT)` : identifier;
}

const sqlOperator = (op: TraceValueOperator) => (op === '!=' ? '<>' : op);

/**
 * The condition, and where in the statement it goes.
 *
 * A statement with a row per trace takes a condition on a span as "some span
 * of the trace has it", or for `!=` "none has it". A statement with a row per
 * span takes it on the span itself.
 */
function sqlCondition(
  base: string,
  field: TraceFieldDef,
  value: string,
  op: TraceValueOperator,
  streamName: string,
): { keyword: 'WHERE' | 'HAVING'; clause: string } {
  const grouped = isGroupedTraceSql(base);
  const literal = sqlLiteral(field, value);
  const aggregate = traceAggregateExpression(field.name);

  if (aggregate && grouped) return { keyword: 'HAVING', clause: `${aggregate} ${sqlOperator(op)} ${literal}` };
  if (aggregate && !field.physical) {
    return { keyword: 'WHERE', clause: traceCountSubquery(streamName, `${aggregate} ${sqlOperator(op)} ${literal}`) };
  }

  const column = sqlColumn(field);
  // A trace's id is the same on all its spans: it is compared before they are grouped.
  if (field.name === 'trace_id') return { keyword: 'WHERE', clause: `${column} ${sqlOperator(op)} ${literal}` };
  if (grouped) {
    return op === '!='
      ? { keyword: 'HAVING', clause: `MAX(CASE WHEN ${column} = ${literal} THEN 1 ELSE 0 END) = 0` }
      : { keyword: 'HAVING', clause: `MAX(CASE WHEN ${column} ${op} ${literal} THEN 1 ELSE 0 END) = 1` };
  }

  // A span without the field is not one that has the value: it stays. The comparison goes in
  // brackets: the engine reads all that follows `IS DISTINCT FROM` as its operand, an AND added
  // after it included.
  return {
    keyword: 'WHERE',
    clause: op === '!=' ? `(${column} IS DISTINCT FROM ${literal})` : `${column} ${op} ${literal}`,
  };
}

// The condition as it stands in a statement: whole, not the start of a longer number or name.
const sqlClausePattern = (clause: string) => `${clause.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}(?![\\w.])`;

/** Adds a condition on a value to a SQL statement; an empty statement starts from the template. */
export function appendTraceSqlValueFilter(
  statement: string,
  field: TraceFieldDef,
  value: string,
  op: TraceValueOperator,
  streamName: string,
): string {
  const base = statement.trim() || traceSqlTemplate(streamName);
  const { keyword, clause } = sqlCondition(base, field, value, op, streamName);

  return new RegExp(sqlClausePattern(clause)).test(base) ? statement : insertTraceSqlClause(base, keyword, clause);
}

/** Whether the statement in the language in use already holds this filter on the value. */
export function hasTraceValueFilter(
  mode: TraceQueryMode,
  statement: string,
  field: TraceFieldDef,
  value: string,
  op: TraceValueOperator,
  streamName: string,
): boolean {
  if (mode === 'fields') {
    const { parts, same } = fieldQueryParts(statement, field, value, op);
    return parts.some(same);
  }

  const base = statement.trim();
  return base !== '' && new RegExp(sqlClausePattern(sqlCondition(base, field, value, op, streamName).clause)).test(base);
}

/** Takes a filter on a value out of the statement again, leaving the rest as it was written. */
export function removeTraceValueFilter(
  mode: TraceQueryMode,
  statement: string,
  field: TraceFieldDef,
  value: string,
  op: TraceValueOperator,
  streamName: string,
): string {
  if (mode === 'fields') {
    const { parts, same } = fieldQueryParts(statement, field, value, op);
    // A value taken out of a list leaves the others asked for.
    return joinFieldConjuncts(
      parts.flatMap((part) => {
        if (!same(part)) return [part];
        const rest = (askedValues(part, field) ?? []).filter((asked) => asked !== value);
        return op === '=' && rest.length > 0 ? [askingFor(field, rest)] : [];
      }),
    );
  }

  const clause = sqlClausePattern(sqlCondition(statement.trim(), field, value, op, streamName).clause);
  // One of several conditions goes with its AND; the only one takes its WHERE or HAVING along.
  const shapes = [`\\s+AND\\s+${clause}`, `${clause}\\s+AND\\s+`, `\\s+(?:WHERE|HAVING)\\s+${clause}`];
  const shape = shapes.map((source) => new RegExp(source, 'i')).find((pattern) => pattern.test(statement));

  return shape ? statement.replace(shape, '') : statement;
}
