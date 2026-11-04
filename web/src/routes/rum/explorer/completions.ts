import type { RumField } from '@/api/rum';
import type { CodeCompletionItem } from '@/shell/codeEditor/types';

import { isRumFieldQueryable } from './fieldQuery';

/** What the field query editor offers while typing: the fields of the type and the comparisons. */
export function rumQueryCompletions(fields: readonly RumField[]): CodeCompletionItem[] {
  return [
    ...fields
      .filter(isRumFieldQueryable)
      .map((field) => field.name)
      .sort()
      .map((label) => ({ label, kind: 'field' as const, detail: 'RUM field' })),
    { label: '=', insertText: '= ', kind: 'operator', detail: 'operator' },
    { label: '!=', insertText: '!= ', kind: 'operator', detail: 'operator' },
    { label: '>=', insertText: '>= ', kind: 'operator', detail: 'operator' },
    { label: '<=', insertText: '<= ', kind: 'operator', detail: 'operator' },
    { label: '>', insertText: '> ', kind: 'operator', detail: 'operator' },
    { label: '<', insertText: '< ', kind: 'operator', detail: 'operator' },
    { label: 'contains', insertText: 'contains ', kind: 'operator', detail: 'operator' },
    { label: 'AND', insertText: 'AND ', kind: 'operator', detail: 'operator' },
  ];
}
