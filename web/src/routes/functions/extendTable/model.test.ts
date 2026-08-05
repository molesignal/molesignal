import { describe, expect, it } from 'vitest';

import { inferValueFields, parseImportText } from './model';

describe('extend table model', () => {
  it('infers stable schema from existing rows', () => {
    expect(
      inferValueFields([
        {
          id: '1',
          org_id: 'o',
          table_name: 'customers',
          key: 'c-1',
          value_json: { tier: 'pro', active: true },
          updated_at_micros: 1,
        },
        {
          id: '2',
          org_id: 'o',
          table_name: 'customers',
          key: 'c-2',
          value_json: { tier: 'free', active: false },
          updated_at_micros: 2,
        },
      ]),
    ).toEqual([
      { name: 'active', field_type: 'boolean', required: true, description: '' },
      { name: 'tier', field_type: 'string', required: true, description: '' },
    ]);
  });

  it('parses JSON arrays using the configured key field', () => {
    expect(
      parseImportText(
        JSON.stringify([
          { customer_id: 'customer-1', tier: 'pro' },
          { customer_id: 'customer-2', tier: 'free' },
        ]),
        'customer_id',
      ),
    ).toEqual([
      { key: 'customer-1', value: { tier: 'pro' } },
      { key: 'customer-2', value: { tier: 'free' } },
    ]);
  });

  it('parses CSV and removes the key column from values', () => {
    expect(
      parseImportText('customer_id,tier,owner\ncustomer-1,pro,platform', 'customer_id'),
    ).toEqual([
      {
        key: 'customer-1',
        value: { tier: 'pro', owner: 'platform' },
      },
    ]);
  });
});
