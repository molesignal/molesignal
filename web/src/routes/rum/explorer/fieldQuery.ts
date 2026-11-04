import type { RumField, RumFilter } from '@/api/rum';
import { appendLogFieldClause, parseLogFieldStatement } from '@/routes/logs/fieldQueryModel';

/** A field query read against the fields of a type. */
export interface ParsedRumStatement {
  filters: RumFilter[];
  /** Words to find in the columns text is looked for in. */
  freeText: string[];
  /** Parts the type cannot answer: a field it does not have, a function that is not written out. */
  rejected: string[];
}

export const NO_STATEMENT: ParsedRumStatement = { filters: [], freeText: [], rejected: [] };

/** Whether a filter on the field can be written: a JSON document has nothing to compare. */
export function isRumFieldQueryable(field: RumField): boolean {
  return field.dataType !== 'json';
}

/** The JSON documents whose keys can be asked for by name, as `feature_flags.<key>`. */
const DOCUMENTS_OF_KEYS = ['feature_flags', 'context', 'view.custom_timings', 'usr.extra'];

/** Whether a name is a key of a document the type has. */
export function isRumMember(name: string, fields: readonly RumField[]): boolean {
  return DOCUMENTS_OF_KEYS.some((document) => name.startsWith(`${document}.`) && name.length > document.length + 1 && fields.some((field) => field.name === document));
}

/**
 * The comparisons and words of a field query. The statement only takes fields the type has, the keys
 * of its documents, and not a JSON document as a whole; the parts that name others are returned as
 * written.
 */
export function parseRumStatement(statement: string, fields: readonly RumField[]): ParsedRumStatement {
  const parsed = parseLogFieldStatement(statement);
  const queryable = new Set(fields.filter(isRumFieldQueryable).map((field) => field.name));
  const filters: RumFilter[] = [];
  const rejected = [...parsed.rejected];

  for (const clause of parsed.filters) {
    if (queryable.has(clause.field) || isRumMember(clause.field, fields)) filters.push(clause);
    else rejected.push(`${clause.field} ${clause.op} ${clause.value}`);
  }

  return { filters, freeText: parsed.freeText, rejected };
}

/** Starts a comparison on a field in the statement, for the value to be typed. */
export function insertRumClause(current: string, field: RumField): string {
  return appendLogFieldClause(current, {
    name: field.name,
    type: 'string',
    dataType: field.dataType,
    count: '0',
    expandable: false,
  });
}
