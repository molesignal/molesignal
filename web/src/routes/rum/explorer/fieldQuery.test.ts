import { describe, expect, it } from 'vitest';

import type { RumField } from '@/api/rum';

import { insertRumClause, isRumFieldQueryable, parseRumStatement } from './fieldQuery';

const field = (name: string, data_type: RumField['dataType'] = 'utf8'): RumField => ({
  name,
  dataType: data_type,
  unit: 'none',
  facetable: data_type !== 'json',
  indexed: false,
});

const FIELDS = [field('usr.id'), field('resource.status_code', 'int64'), field('context', 'json'), field('feature_flags', 'json')];

describe('parseRumStatement', () => {
  it('reads a comparison of a key of a JSON field, and sends back the whole field', () => {
    const parsed = parseRumStatement("feature_flags.checkout_v2 = 'on' AND context contains 'x'", FIELDS);

    expect(parsed.filters).toEqual([{ field: 'feature_flags.checkout_v2', op: '=', value: 'on', quoted: true }]);
    expect(parsed.rejected).toEqual(['context contains x']);
  });

  it('reads comparisons and words against the fields of the type', () => {
    const parsed = parseRumStatement("usr.id = 'u-42' AND resource.status_code >= 500 AND timeout", FIELDS);

    expect(parsed.filters).toEqual([
      { field: 'usr.id', op: '=', value: 'u-42', quoted: true },
      { field: 'resource.status_code', op: '>=', value: '500', quoted: false },
    ]);
    expect(parsed.freeText).toEqual(['timeout']);
    expect(parsed.rejected).toEqual([]);
  });

  it('sends back what the type cannot answer: a field it lacks, a document', () => {
    const parsed = parseRumStatement("nope = 'x' AND context contains 'a' AND usr.id != ''", FIELDS);

    expect(parsed.filters.map((filter) => filter.field)).toEqual(['usr.id']);
    expect(parsed.rejected).toEqual(["nope = x", 'context contains a']);
  });

  it('keeps an empty value as a question about the field having one', () => {
    expect(parseRumStatement("usr.id = ''", FIELDS).filters).toEqual([
      { field: 'usr.id', op: '=', value: '', quoted: true },
    ]);
  });

  it('reads nothing from nothing', () => {
    expect(parseRumStatement('  ', FIELDS)).toEqual({ filters: [], freeText: [], rejected: [] });
  });
});

describe('insertRumClause', () => {
  it('starts a comparison for the value to be typed, by the type of the field', () => {
    expect(insertRumClause('', field('usr.id'))).toBe("usr.id = ''");
    expect(insertRumClause('', field('resource.status_code', 'int64'))).toBe('resource.status_code >= 0');
    expect(insertRumClause("a = 'b'", field('view.is_active', 'bool'))).toBe("a = 'b' AND view.is_active = true");
  });

  it('does not start a second comparison on a field that has one', () => {
    expect(insertRumClause("usr.id = 'u'", field('usr.id'))).toBe("usr.id = 'u'");
  });
});

describe('isRumFieldQueryable', () => {
  it('is false for a document only', () => {
    expect(isRumFieldQueryable(field('context', 'json'))).toBe(false);
    expect(isRumFieldQueryable(field('usr.id'))).toBe(true);
  });
});
