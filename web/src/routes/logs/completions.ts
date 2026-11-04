import type { CodeCompletionItem } from '@/shell/codeEditor/types';

import type { LogFieldDef } from './fieldQueryModel';
import type { LogEntry } from './viewModel';

// The values are read from the first logs of the page, at most this many of each field.
const VALUE_ROWS = 200;
const FIELD_VALUES = 50;
// A longer string is something a log says, not something a query compares a field with.
const VALUE_LENGTH = 80;

const OPERATORS: readonly CodeCompletionItem[] = [
  { label: '=', insertText: '= ', kind: 'operator', detail: 'operator' },
  { label: '!=', insertText: '!= ', kind: 'operator', detail: 'operator' },
  { label: '>=', insertText: '>= ', kind: 'operator', detail: 'operator' },
  { label: '<=', insertText: '<= ', kind: 'operator', detail: 'operator' },
  { label: '>', insertText: '> ', kind: 'operator', detail: 'operator' },
  { label: '<', insertText: '< ', kind: 'operator', detail: 'operator' },
  { label: 'contains', insertText: 'contains ', kind: 'operator', detail: 'operator' },
  { label: 'AND', insertText: 'AND ', kind: 'operator', detail: 'operator' },
  { label: 'OR', insertText: 'OR ', kind: 'operator', detail: 'operator' },
];

function quoted(value: string): string {
  return `"${value.replace(/\\/g, '\\\\').replace(/"/g, '\\"')}"`;
}

/**
 * What the field query editor offers while typing: the search functions, the
 * stream's fields, the comparisons and the values the loaded logs hold. A name
 * or a value the stream does not have is never offered: the query refuses a
 * field the stream lacks, and a made-up value finds nothing.
 *
 * Values are offered once the stream's masking is known, and never those of a
 * masked field: the editor must not show what the list hides.
 */
export function logQueryCompletions(
  functions: readonly CodeCompletionItem[],
  fields: readonly Pick<LogFieldDef, 'name'>[],
  rows: readonly Pick<LogEntry, 'raw'>[],
  maskedFields: ReadonlySet<string> | null,
): CodeCompletionItem[] {
  const names = new Set(fields.map((field) => field.name));
  const values = maskedFields ? loadedValues(rows, names, maskedFields) : new Map<string, Set<string>>();

  return [
    ...functions,
    ...[...names].sort().map((label) => ({ label, kind: 'field' as const, detail: 'log field' })),
    ...OPERATORS,
    ...[...values].flatMap(([field, fieldValues]) => (
      [...fieldValues].sort().map((value) => {
        const text = quoted(value);
        return {
          label: text,
          insertText: text,
          kind: 'value' as const,
          detail: `${field} value`,
          field,
          value,
        };
      })
    )),
  ];
}

/** The values each unmasked field of the stream has in the loaded logs, as they are written. */
function loadedValues(
  rows: readonly Pick<LogEntry, 'raw'>[],
  names: ReadonlySet<string>,
  maskedFields: ReadonlySet<string>,
): Map<string, Set<string>> {
  const values = new Map<string, Set<string>>();

  for (const row of rows.slice(0, VALUE_ROWS)) {
    for (const [field, value] of Object.entries(row.raw)) {
      // Masking names its fields in lower case.
      if (!names.has(field) || maskedFields.has(field.toLowerCase())) continue;
      const text = comparedText(value);
      if (text === null) continue;
      const fieldValues = values.get(field) ?? new Set<string>();
      if (fieldValues.size < FIELD_VALUES) fieldValues.add(text);
      values.set(field, fieldValues);
    }
  }

  return values;
}

function comparedText(value: unknown): string | null {
  if (typeof value === 'string') return value.length > 0 && value.length <= VALUE_LENGTH ? value : null;
  if (typeof value === 'number' || typeof value === 'boolean') return String(value);

  return null;
}
