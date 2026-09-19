import { describe, expect, it } from 'vitest';

import type { CodeCompletionItem } from './types';
import {
  filterFieldQueryCompletionItems,
  presentFieldQueryCompletion,
  resolveFieldQueryCompletionContext,
} from './fieldQueryCompletion';

const items: CodeCompletionItem[] = [
  { label: 'level', kind: 'field' },
  { label: 'service_name', kind: 'field' },
  { label: 'MATCH', kind: 'function' },
  {
    label: '"INFO"',
    insertText: '"INFO"',
    kind: 'value',
    field: 'level',
    value: 'INFO',
  },
  {
    label: '"checkout"',
    insertText: '"checkout"',
    kind: 'value',
    field: 'service_name',
    value: 'checkout',
  },
];

describe('Fields query completion context', () => {
  it('offers only fields for the first MATCH argument', () => {
    const context = resolveFieldQueryCompletionContext('MATCH(le');

    expect(context).toEqual({ kind: 'field' });
    expect(filterFieldQueryCompletionItems(items, context).map((item) => item.label))
      .toEqual(['level', 'service_name']);
  });

  it('offers values belonging to the MATCH field for the term argument', () => {
    const context = resolveFieldQueryCompletionContext("MATCH(level, 'INF");

    expect(context).toEqual({ kind: 'value', field: 'level', quote: "'" });
    expect(filterFieldQueryCompletionItems(items, context).map((item) => item.label))
      .toEqual(['"INFO"']);
  });

  it('uses the same field-aware value context for normal comparisons', () => {
    expect(resolveFieldQueryCompletionContext('service_name = "check'))
      .toEqual({ kind: 'value', field: 'service_name', quote: '"' });
  });

  it('inserts only the raw value inside an existing quote', () => {
    const context = resolveFieldQueryCompletionContext("MATCH(level, 'INF");
    const completion = items.find((item) => item.value === 'INFO')!;

    expect(presentFieldQueryCompletion(completion, context)).toEqual({
      label: 'INFO',
      insertText: 'INFO',
      advanceSnippet: false,
    });
  });

  it('advances to the term placeholder after accepting the MATCH field', () => {
    const context = resolveFieldQueryCompletionContext('MATCH(le');
    const completion = items.find((item) => item.label === 'level')!;

    expect(presentFieldQueryCompletion(completion, context)).toEqual({
      label: 'level',
      insertText: 'level',
      advanceSnippet: true,
    });
  });

  it('returns to expression completions after a closed function call', () => {
    const context = resolveFieldQueryCompletionContext(
      "MATCH(level, 'INFO') AND ser",
    );

    expect(context).toEqual({ kind: 'expression' });
    expect(filterFieldQueryCompletionItems(items, context).map((item) => item.label))
      .toEqual(['level', 'service_name', 'MATCH']);
  });
});
