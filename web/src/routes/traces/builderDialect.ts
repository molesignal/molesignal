import type { FieldType } from '@/api/streams';
import {
  type BuilderClause,
  type BuilderDialect,
  type BuilderField,
  type BuilderFieldKind,
  isBareValue,
  operatorsForKind,
  TEXT_OPERATORS,
} from '@/shell/query/builder/model';

import { isTraceFieldQueryable, parseTraceStatement, type TraceFieldDef } from './fieldQueryModel';
import { quoteTraceValue } from './urlState';

function fieldKind(dataType: FieldType): BuilderFieldKind {
  if (dataType === 'bool') return 'boolean';
  if (dataType === 'utf8') return 'text';
  return 'number';
}

/**
 * The fields a filter can be written on. A field query cannot compare a JSON
 * column, and names its fields bare, so it does not reach one whose name is
 * not a single word.
 */
export function traceBuilderFields(fields: readonly TraceFieldDef[]): BuilderField[] {
  return fields
    .filter((field) => isTraceFieldQueryable(field, 'fields'))
    .map((field) => ({ name: field.name, kind: fieldKind(field.dataType) }));
}

/**
 * The Traces field query, read and written as the filters the builder shows.
 * It is made for a schema: the statement only takes fields the stream has, and
 * a comparison the field's type does not support is kept as written.
 */
export function traceBuilderDialect(fields: TraceFieldDef[]): BuilderDialect {
  const kinds = new Map(traceBuilderFields(fields).map((field) => [field.name, field.kind]));

  const writeClause = (clause: BuilderClause): string => {
    const value = isBareValue(kinds.get(clause.field), clause.value)
      ? clause.value.trim()
      : `'${quoteTraceValue(clause.value)}'`;
    return `${clause.field} ${clause.op} ${value}`;
  };

  return {
    parse(statement) {
      const parsed = parseTraceStatement(statement, fields);
      return {
        clauses: parsed.filters.map((filter) => ({
          field: filter.field,
          op: filter.op,
          value: filter.value,
        })),
        text: parsed.q ?? '',
        unsupported: parsed.rejected,
      };
    },
    serialize({ clauses, text, unsupported }) {
      return [...clauses.map(writeClause), ...unsupported, text.trim()]
        .filter(Boolean)
        .join(' AND ');
    },
    operators(field) {
      return field ? operatorsForKind(field.kind) : TEXT_OPERATORS;
    },
  };
}
