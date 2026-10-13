import type { FieldAnalysisKind } from '@/shell/query/fieldAnalysis/model';

import type { LogFieldDef } from '../fieldQueryModel';

/**
 * How a log field's values are read when the field is analysed. `null` for a
 * field of documents or lists: a document per record has no values to rank or
 * to measure, and its paths are asked for in SQL.
 */
export function logFieldAnalysisKind(field: Pick<LogFieldDef, 'type' | 'dataType'>): FieldAnalysisKind | null {
  if (field.dataType === 'json' || field.type === 'object' || field.type === 'array') return null;

  switch (field.dataType) {
    case 'int64':
    case 'float64':
      return 'number';
    case 'timestamp':
      return 'time';
    case 'bool':
      return 'bool';
    default:
      return 'text';
  }
}
