import type { FieldType } from '@/api/streams';
import {
  BOOLEAN_OPERATORS,
  type BuilderClause,
  type BuilderDialect,
  type BuilderField,
  type BuilderFieldKind,
  type BuilderOperator,
  NUMBER_OPERATORS,
} from '@/shell/query/builder/model';

import { type LogFieldDef, parseLogFieldStatement } from './fieldQueryModel';

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

/** The Logs field query, read and written as the filters the builder shows. */
export const logBuilderDialect: BuilderDialect = {
  parse(statement) {
    const parsed = parseLogFieldStatement(statement);
    return {
      clauses: parsed.filters.map((filter) => ({
        field: filter.field,
        op: filter.op,
        value: filter.value,
        quoted: filter.quoted,
      })),
      text: parsed.freeText.join(' AND '),
      unsupported: parsed.rejected,
    };
  },
  serialize({ clauses, text, unsupported }) {
    return [...clauses.map(writeClause), ...unsupported, text.trim()]
      .filter(Boolean)
      .join(' AND ');
  },
  operators(field) {
    if (!field) return LOG_ANY_OPERATORS;
    if (field.kind === 'number') return NUMBER_OPERATORS;
    if (field.kind === 'boolean') return BOOLEAN_OPERATORS;
    return LOG_TEXT_OPERATORS;
  },
};
