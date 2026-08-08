import { describe, expect, it } from 'vitest';

import type { SqlQueryCapabilities } from '@/api/query';

import { resolveSqlFunctionCompletions } from './useSqlFunctionCompletions';

describe('SQL function completions', () => {
  it('provides uppercase built-ins while capabilities are unavailable', () => {
    const completions = resolveSqlFunctionCompletions(undefined);

    expect(completions.map((item) => item.label)).toEqual(['MATCH', 'MATCH_TEXT']);
    expect(completions.map((item) => item.insertText)).toEqual([
      "MATCH(${1:field}, '${2:term}')",
      "MATCH_TEXT(${1:field}, '${2:query}')",
    ]);
    expect(completions.every((item) => item.kind === 'function')).toBe(true);
    expect(completions.every((item) => item.insertTextFormat === 'snippet')).toBe(true);
  });

  it('uses the server list and normalizes labels and inserted function names', () => {
    const capabilities: SqlQueryCapabilities = {
      engine: 'molesignal-sql',
      version: 1,
      functions: [
        {
          label: 'match',
          insert_text: "match(${1:field}, '${2:term}')",
          detail: 'detail',
          documentation: 'documentation',
          kind: 'function',
        },
      ],
    };

    expect(resolveSqlFunctionCompletions(capabilities)).toEqual([
      {
        label: 'MATCH',
        insertText: "MATCH(${1:field}, '${2:term}')",
        insertTextFormat: 'snippet',
        kind: 'function',
        detail: 'detail',
        documentation: 'documentation',
      },
    ]);
  });

  it('respects an authoritative empty server capability list', () => {
    expect(resolveSqlFunctionCompletions({
      engine: 'molesignal-sql',
      version: 1,
      functions: [],
    })).toEqual([]);
  });
});
