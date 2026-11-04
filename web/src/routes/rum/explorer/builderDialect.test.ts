import { describe, expect, it } from 'vitest';

import type { RumField } from '@/api/rum';

import { rumBuilderDialect, rumBuilderFields } from './builderDialect';

const field = (name: string, data_type: RumField['dataType']): RumField => ({
  name,
  dataType: data_type,
  unit: 'none',
  facetable: true,
  indexed: false,
});

describe('the builder of the explorer', () => {
  it('offers the fields a comparison can be written on, by the kind of their values', () => {
    expect(
      rumBuilderFields([
        field('usr.id', 'utf8'),
        field('resource.duration', 'int64'),
        field('view.is_active', 'bool'),
        field('context', 'json'),
      ]),
    ).toEqual([
      { name: 'usr.id', kind: 'text' },
      { name: 'resource.duration', kind: 'number' },
      { name: 'view.is_active', kind: 'boolean' },
    ]);
  });

  it('reads a field query as filters and writes it back', () => {
    const statement = "usr.id = 'u-42' AND resource.status_code >= 500 AND timeout";
    const parsed = rumBuilderDialect.parse(statement);

    expect(parsed.groups[0]!.clauses.map((clause) => `${clause.field} ${clause.op} ${clause.value}`)).toEqual([
      'usr.id = u-42',
      'resource.status_code >= 500',
    ]);
    expect(parsed.groups[0]!.text).toBe('timeout');
    expect(rumBuilderDialect.serialize(parsed)).toBe(statement);
  });

  it('takes the comparisons a kind of field answers', () => {
    expect(rumBuilderDialect.operators({ name: 'a', kind: 'text' })).toEqual(['=', '!=', 'contains']);
    expect(rumBuilderDialect.operators({ name: 'a', kind: 'number' })).toContain('>=');
    expect(rumBuilderDialect.operators({ name: 'a', kind: 'boolean' })).toEqual(['=', '!=']);
    expect(rumBuilderDialect.operators(undefined)).toContain('contains');
  });
});
