import type { FieldType } from '@/api/streams';
import {
  BOOLEAN_OPERATORS,
  type BuilderClause,
  type BuilderDialect,
  type BuilderField,
  type BuilderFieldKind,
  type BuilderOperator,
  type BuilderPart,
  NUMBER_OPERATORS,
  readBuilderStatement,
  writeBuilderStatement,
} from '@/shell/query/builder/model';

import { type LogFieldDef, readLogFieldPart } from './fieldQueryModel';

// Text can also be searched with the index-backed functions.
const LOG_TEXT_OPERATORS: readonly BuilderOperator[] = ['=', '!=', 'contains', 'match', 'match_text'];
// A field the schema does not list could be anything, so every comparison is offered.
const LOG_ANY_OPERATORS: readonly BuilderOperator[] = [
  '=',
  '!=',
  'contains',
  '>',
  '>=',
  '<',
  '<=',
  'match',
  'match_text',
];

function fieldKind(dataType: FieldType): BuilderFieldKind {
  if (dataType === 'bool') return 'boolean';
  if (dataType === 'int64' || dataType === 'float64' || dataType === 'timestamp') return 'number';
  return 'text';
}

/** The fields a filter can be written on; JSON columns cannot be compared in a field query. */
export function logBuilderFields(fields: readonly LogFieldDef[]): BuilderField[] {
  return fields
    .filter((field) => field.dataType !== 'json')
    .map((field) => ({ name: field.name, kind: fieldKind(field.dataType) }));
}

function quote(value: string): string {
  return `'${value
    .replace(/\\/g, '\\\\')
    .replace(/'/g, "\\'")
    .replace(/\n/g, '\\n')
    .replace(/\r/g, '\\r')
    .replace(/\t/g, '\\t')}'`;
}

function writeClause(clause: BuilderClause): string {
  if (clause.op === 'match' || clause.op === 'match_text') {
    return `${clause.op.toUpperCase()}(${clause.field}, ${quote(clause.value)})`;
  }
  const value = clause.quoted === false ? clause.value : quote(clause.value);
  return `${clause.field} ${clause.op} ${value}`;
}

/** A part of the statement as the builder shows it: text to find as it was written, quotes and all. */
function readPart(text: string): BuilderPart {
  const read = readLogFieldPart(text);
  if (read.kind === 'filter') {
    const { field, op, value, quoted } = read.filter;
    return { kind: 'clause', clause: { field, op, value, quoted } };
  }
  return read.kind === 'text' ? { kind: 'text', text } : { kind: 'unsupported', text };
}

function dialect(alternatives: boolean): BuilderDialect {
  return {
    alternatives,
    parse: (statement) => readBuilderStatement(statement, readPart, { alternatives, joinText: ' AND ' }),
    serialize: (statement) => writeBuilderStatement(statement, writeClause),
    operators(field) {
      if (!field) return LOG_ANY_OPERATORS;
      if (field.kind === 'number') return NUMBER_OPERATORS;
      if (field.kind === 'boolean') return BOOLEAN_OPERATORS;
      return LOG_TEXT_OPERATORS;
    },
  };
}

/** The Logs field query, read and written as rows of filters: one row for each alternative. */
export const logBuilderDialect = dialect(true);

/**
 * The same field query as one row of filters, for a page whose reads take no
 * alternatives: a statement with an OR outside brackets is kept as written.
 */
export const logFilterListDialect = dialect(false);
