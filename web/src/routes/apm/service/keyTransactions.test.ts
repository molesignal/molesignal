import { describe, expect, it } from 'vitest';

import type { ServiceDefinition } from '@/api/apm';

import { transactionKey, withDefinition } from './keyTransactions';

const definition = (name: string, patch: Partial<ServiceDefinition> = {}): ServiceDefinition => ({
  namespace: 'shop',
  name,
  description: '',
  updatedBy: 'u',
  updatedAt: 0,
  ...patch,
});

describe('key transactions', () => {
  it('names an endpoint by its kind and name', () => {
    expect(transactionKey({ kind: 'http', name: 'GET /cart' })).toBe('http:GET /cart');
  });

  it('puts a saved definition in place of the one it replaces, or adds it', () => {
    const starred = definition('cart', { keyTransactions: [{ kind: 'http', name: 'GET /cart' }] });

    expect(withDefinition([definition('cart'), definition('checkout')], starred).map((entry) => [entry.name, entry.keyTransactions?.length ?? 0])).toEqual([
      ['checkout', 0],
      ['cart', 1],
    ]);
    expect(withDefinition(undefined, starred)).toEqual([starred]);
  });
});
