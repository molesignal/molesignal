import type { FieldType } from '@/api/streams';
import {
  type BuilderClause,
  type BuilderDialect,
  type BuilderField,
  type BuilderFieldKind,
  type BuilderPart,
  isBareValue,
  operatorsForKind,
  readBuilderStatement,
  TEXT_OPERATORS,
  writeBuilderStatement,
} from '@/shell/query/builder/model';

import { isTraceFieldQueryable, readTracePart, type TraceFieldDef } from './fieldQueryModel';
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
 * The Traces field query, read and written as rows of filters, one row for
 * each alternative. It is made for a schema: the statement only takes fields
 * the stream has, and a comparison the field's type does not support, or a
 * list of values, is kept as written.
 */
export function traceBuilderDialect(fields: TraceFieldDef[]): BuilderDialect {
  const kinds = new Map(traceBuilderFields(fields).map((field) => [field.name, field.kind]));

  const writeClause = (clause: BuilderClause): string => {
    const value = isBareValue(kinds.get(clause.field), clause.value)
      ? clause.value.trim()
      : `'${quoteTraceValue(clause.value)}'`;
    return `${clause.field} ${clause.op} ${value}`;
  };

  const readPart = (text: string): BuilderPart => {
    const read = readTracePart(text, fields);
    // A list of values is no single filter of the builder: it is kept as written.
    if (read.kind === 'filter' && read.filter.op !== 'in') {
      return { kind: 'clause', clause: { field: read.filter.field, op: read.filter.op, value: read.filter.value } };
    }
    return read.kind === 'text' ? { kind: 'text', text } : { kind: 'unsupported', text };
  };

  return {
    alternatives: true,
    // Free text of a row is one phrase, as the trace list reads it.
    parse: (statement) => readBuilderStatement(statement, readPart, { alternatives: true, joinText: ' ' }),
    serialize: (statement) => writeBuilderStatement(statement, writeClause),
    operators(field) {
      return field ? operatorsForKind(field.kind) : TEXT_OPERATORS;
    },
  };
}
