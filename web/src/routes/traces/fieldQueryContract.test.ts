import { describe, expect, it } from 'vitest';

import type { StreamField } from '@/api/streams';

import cases from '../../../../contracts/field-query/cases.json';

import { deriveTraceFields, parseTraceStatement } from './fieldQueryModel';

interface ContractCase {
  query: string;
  read?: Record<string, unknown>;
  error?: boolean;
}

const field = (name: string, data_type: StreamField['dataType']): StreamField => ({
  name,
  dataType: data_type,
  nullable: true,
  indexed: false,
});

// A stream that takes every field and operator the cases use: the server checks
// those against the stream when the read validates its filters.
const fields = deriveTraceFields([
  field('trace_id', 'utf8'),
  field('service.name', 'utf8'),
  field('name', 'utf8'),
  field('status_code', 'utf8'),
  field('duration_ns', 'int64'),
]);

// The server reads `expression` from the same table (`contracts/field-query/`).
describe('the field query contract for traces', () => {
  for (const contract of cases.traces as ContractCase[]) {
    it(`reads ${JSON.stringify(contract.query)}`, () => {
      const { q, filters, where, rejected } = parseTraceStatement(contract.query, fields);

      if (contract.error) {
        expect(rejected.length).toBeGreaterThan(0);
        return;
      }

      expect(rejected).toEqual([]);
      expect({ ...(q ? { q } : {}), filters, ...(where ? { where } : {}) }).toEqual(contract.read);
    });
  }
});
