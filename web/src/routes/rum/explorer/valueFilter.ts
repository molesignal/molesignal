import type { RumField, RumFieldType } from '@/api/rum';
import { unquoteLogFieldValue } from '@/routes/logs/fieldQueryModel';
import { isBareValue, type BuilderFieldKind } from '@/shell/query/builder/model';

import { isRumFieldQueryable } from './fieldQuery';
import { quoteRumValue } from './urlState';

/** What a value picked in the field list can be asked of: it, not it, at least it, at most it. */
export type RumValueOperator = '=' | '!=' | '>=' | '<=';

// The longest value a filter takes, in bytes.
const MAX_FILTER_VALUE_BYTES = 512;

const isNumber = (type: RumFieldType) => type === 'int64' || type === 'float64' || type === 'timestamp';

function kindOf(type: RumFieldType): BuilderFieldKind {
  if (type === 'bool') return 'boolean';
  return isNumber(type) ? 'number' : 'text';
}

/** Whether the value is one the field's type can be compared with. */
function isValueOfType(type: RumFieldType, value: string): boolean {
  if (type === 'int64' || type === 'timestamp') return /^-?\d+$/.test(value);
  if (type === 'float64') return value.trim() !== '' && Number.isFinite(Number(value));
  if (type === 'bool') return /^(true|false)$/i.test(value);
  return type === 'utf8';
}

/** Whether a filter on this value can be written, and mean what the button says. */
export function canFilterRumValue(field: RumField, value: string, op: RumValueOperator): boolean {
  if (!isRumFieldQueryable(field) || !isValueOfType(field.dataType, value)) return false;
  if ((op === '>=' || op === '<=') && !isNumber(field.dataType)) return false;

  // The server trims a value and caps its length, so one that starts or ends with a space, or
  // runs long, would never match itself.
  return (
    value !== '' &&
    value === value.trim() &&
    !value.includes('\0') &&
    new TextEncoder().encode(value).length <= MAX_FILTER_VALUE_BYTES
  );
}

interface Clause {
  field: string;
  op: string;
  value: string;
}

/** The parts of a field query, joined by AND outside quotes. */
export function splitRumStatement(statement: string): string[] {
  const parts: string[] = [];
  let start = 0;
  let quote: "'" | '"' | null = null;
  let escaped = false;

  for (let index = 0; index < statement.length; index += 1) {
    const character = statement[index]!;

    if (escaped) {
      escaped = false;
    } else if (character === '\\') {
      escaped = true;
    } else if (quote) {
      if (character === quote) quote = null;
    } else if (character === "'" || character === '"') {
      quote = character;
    } else {
      const conjunction = statement.slice(index).match(/^\s+AND\s+/i);
      if (conjunction) {
        parts.push(statement.slice(start, index));
        index += conjunction[0].length - 1;
        start = index + 1;
      }
    }
  }

  parts.push(statement.slice(start));
  return parts.map((part) => part.trim()).filter(Boolean);
}

function clauseOf(part: string): Clause | null {
  const match = part.match(/^([a-zA-Z_][\w.]*)\s*(>=|<=|!=|=|>|<|contains\b)\s*([\s\S]*)$/i);
  if (!match) return null;

  return { field: match[1]!, op: match[2]!.toLowerCase(), value: unquoteLogFieldValue(match[3]!).value };
}

function literal(field: RumField, value: string): string {
  return isBareValue(kindOf(field.dataType), value) ? value.trim() : `'${quoteRumValue(value)}'`;
}

const boundOf = (op: string): 'lower' | 'upper' | null => {
  if (op === '>' || op === '>=') return 'lower';
  if (op === '<' || op === '<=') return 'upper';
  return null;
};

/**
 * Whether a comparison already in the statement gives way to the one being added: the empty one
 * the field's "+" left, another value of the same field asked for in the same way, the same value
 * asked the other way round, an earlier bound on the same side.
 */
function gaveWay(existing: Clause, field: RumField, op: RumValueOperator, value: string): boolean {
  if (existing.field !== field.name) return false;
  if (existing.value === '') return true;

  const equality = (candidate: string) => candidate === '=' || candidate === '!=';

  if (equality(op) && equality(existing.op)) {
    return existing.value === value || (op === '=' && existing.op === '=');
  }

  return boundOf(op) !== null && boundOf(op) === boundOf(existing.op);
}

const sameClause = (existing: Clause | null, field: RumField, op: RumValueOperator, value: string) =>
  existing?.field === field.name && existing.op === op && existing.value === value;

/**
 * Adds a comparison on a value to a field query. The statement is kept as it was written, but for
 * the comparisons the new one answers (see [`gaveWay`]), which it takes the place of.
 */
export function insertRumValueClause(
  current: string,
  field: RumField,
  value: string,
  op: RumValueOperator,
): string {
  const clause = `${field.name} ${op} ${literal(field, value)}`;
  const parts = splitRumStatement(current.replace(/\s+AND\s*$/i, ''));

  if (parts.some((part) => sameClause(clauseOf(part), field, op, value))) return current;

  const next: string[] = [];
  let placed = false;

  for (const part of parts) {
    const existing = clauseOf(part);

    if (existing === null || !gaveWay(existing, field, op, value)) {
      next.push(part);
    } else if (!placed) {
      next.push(clause);
      placed = true;
    }
  }

  return (placed ? next : [...next, clause]).join(' AND ');
}

/** Whether the statement already holds this filter on the value. */
export function hasRumValueFilter(
  statement: string,
  field: RumField,
  value: string,
  op: RumValueOperator,
): boolean {
  return splitRumStatement(statement).some((part) => sameClause(clauseOf(part), field, op, value));
}

/** Takes a filter on a value out of the statement again, leaving the rest as it was written. */
export function removeRumValueFilter(
  statement: string,
  field: RumField,
  value: string,
  op: RumValueOperator,
): string {
  return splitRumStatement(statement)
    .filter((part) => !sameClause(clauseOf(part), field, op, value))
    .join(' AND ');
}
