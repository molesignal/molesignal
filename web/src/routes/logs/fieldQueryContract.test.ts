import { describe, expect, it } from 'vitest';

import cases from '../../../../contracts/field-query/cases.json';

import { logMatchRequest } from './cursorQuery';

interface ContractCase {
  query: string;
  read?: Record<string, unknown>;
  error?: boolean;
}

// The server reads `expression` from the same table (`contracts/field-query/`).
describe('the field query contract for logs', () => {
  for (const contract of cases.logs as ContractCase[]) {
    it(`reads ${JSON.stringify(contract.query)}`, () => {
      const read = () => logMatchRequest('app', contract.query, [], { start: 0, end: 1 });

      if (contract.error) {
        expect(read).toThrow();
        return;
      }

      const { filters, freeText, where } = read();
      expect({ filters, freeText, ...(where ? { where } : {}) }).toEqual(contract.read);
    });
  }
});
