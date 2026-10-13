import { describe, expect, it } from 'vitest';

import type { FunctionResp } from '@/api/functions';

import { copyName, languageName, startFromPath, viewCounts, visibleFunctions } from './model';

function fn(name: string, patch: Partial<FunctionResp> = {}): FunctionResp {
  return {
    id: `id-${name}`,
    name,
    language: 'vrl',
    source: '.a = 1',
    params_schema: {},
    is_builtin: false,
    description: '',
    created_at_micros: 1,
    updated_at_micros: 2,
    ...patch,
  };
}

const FUNCTIONS = [
  fn('mask-cards'),
  fn('to-celsius', { language: 'js' }),
  fn('redact-email', { is_builtin: true, description: 'Replaces email addresses with a placeholder.' }),
  fn('normalize-logs', { is_builtin: true, description: 'Parses the JSON message.' }),
];

describe('viewCounts', () => {
  it('counts every view before any search', () => {
    expect(viewCounts(FUNCTIONS)).toEqual({ all: 4, own: 2, builtin: 2 });
    expect(viewCounts([])).toEqual({ all: 0, own: 0, builtin: 0 });
  });
});

describe('visibleFunctions', () => {
  it('keeps the functions of the view', () => {
    expect(visibleFunctions(FUNCTIONS, 'all', '').map((item) => item.name)).toEqual([
      'mask-cards',
      'to-celsius',
      'redact-email',
      'normalize-logs',
    ]);
    expect(visibleFunctions(FUNCTIONS, 'own', '').map((item) => item.name)).toEqual([
      'mask-cards',
      'to-celsius',
    ]);
    expect(visibleFunctions(FUNCTIONS, 'builtin', '').map((item) => item.name)).toEqual([
      'redact-email',
      'normalize-logs',
    ]);
  });

  it('finds a function by its name, whatever the case', () => {
    expect(visibleFunctions(FUNCTIONS, 'all', ' MASK ').map((item) => item.name)).toEqual([
      'mask-cards',
    ]);
  });

  it('finds a preset by what it is said to do', () => {
    expect(visibleFunctions(FUNCTIONS, 'all', 'placeholder').map((item) => item.name)).toEqual([
      'redact-email',
    ]);
  });

  it('searches inside the view only', () => {
    expect(visibleFunctions(FUNCTIONS, 'own', 'redact')).toEqual([]);
  });
});

describe('languageName', () => {
  it('writes a language as it is written', () => {
    expect(languageName('vrl')).toBe('VRL');
    expect(languageName('js')).toBe('JavaScript');
  });
});

describe('startFromPath', () => {
  it('names the function a new one starts from', () => {
    expect(startFromPath({ id: 'a b/c' })).toBe('/functions/new?from=a%20b%2Fc');
  });
});

describe('copyName', () => {
  it('adds -copy to the name', () => {
    expect(copyName('redact-email', ['mask-cards'])).toBe('redact-email-copy');
  });

  it('counts on when a copy of that name exists', () => {
    expect(copyName('redact-email', ['redact-email-copy'])).toBe('redact-email-copy-2');
    expect(copyName('redact-email', ['redact-email-copy', 'redact-email-copy-2'])).toBe(
      'redact-email-copy-3',
    );
  });
});
