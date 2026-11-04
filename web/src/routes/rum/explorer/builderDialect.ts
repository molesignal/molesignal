import type { RumField } from '@/api/rum';
import { logFilterListDialect } from '@/routes/logs/builderDialect';
import {
  BOOLEAN_OPERATORS,
  type BuilderDialect,
  type BuilderField,
  type BuilderFieldKind,
  type BuilderOperator,
  NUMBER_OPERATORS,
} from '@/shell/query/builder/model';

import { isRumFieldQueryable } from './fieldQuery';

const TEXT_OPERATORS: readonly BuilderOperator[] = ['=', '!=', 'contains'];

function fieldKind(field: RumField): BuilderFieldKind {
  if (field.dataType === 'bool') return 'boolean';
  if (field.dataType === 'utf8' || field.dataType === 'json') return 'text';
  return 'number';
}

/** The fields a filter can be written on. */
export function rumBuilderFields(fields: readonly RumField[]): BuilderField[] {
  return fields
    .filter(isRumFieldQueryable)
    .map((field) => ({ name: field.name, kind: fieldKind(field) }));
}

/**
 * The field query of the explorer, read and written as the filters the builder shows. It is the
 * statement the log explorer takes, with the comparisons the rows of a type answer.
 */
export const rumBuilderDialect: BuilderDialect = {
  ...logFilterListDialect,
  operators(field) {
    if (!field) return [...TEXT_OPERATORS, '>', '>=', '<', '<='];
    if (field.kind === 'number') return NUMBER_OPERATORS;
    if (field.kind === 'boolean') return BOOLEAN_OPERATORS;
    return TEXT_OPERATORS;
  },
};
