import { describe, expect, it } from 'vitest';

import {
  formatJsonEditor,
  parseDelimitedList,
  parseJsonEditor,
  stringListValue,
} from './editorModel';

describe('Mole Intelligence editor models', () => {
  it('formats and parses editable JSON without changing its structure', () => {
    const value = { type: 'manual', filters: ['production'] };
    expect(parseJsonEditor(formatJsonEditor(value))).toEqual({ ok: true, value });
  });

  it('returns a useful parse error for invalid JSON', () => {
    const result = parseJsonEditor('{');
    expect(result.ok).toBe(false);
    if (!result.ok) expect(result.message.length).toBeGreaterThan(0);
  });

  it('normalizes comma and newline separated lists', () => {
    expect(parseDelimitedList('production, staging\nproduction')).toEqual([
      'production',
      'staging',
    ]);
    expect(stringListValue(['production', 42, 'staging'])).toBe('production, staging');
  });
});
