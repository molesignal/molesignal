import { describe, expect, it } from 'vitest';

import type { ApiToken } from '@/api/apiTokens';

import { countRisks, tokenRisks } from './risk';

const DAY = 86_400_000_000;
const NOW = 1000 * DAY;

function token(fields: Partial<ApiToken>): ApiToken {
  return {
    id: 't',
    name: 't',
    prefix: 'p',
    tokenKind: 'personal',
    userId: 'u',
    revoked: false,
    createdAt: NOW - DAY,
    expiresAt: NOW + 365 * DAY,
    lastUsedAt: NOW - DAY,
    policyId: 'read-only',
    ...fields,
  } as ApiToken;
}

describe('tokenRisks', () => {
  it('finds nothing in a scoped token in use that ends in good time', () => {
    expect(tokenRisks(token({}), NOW)).toEqual([]);
  });

  it('flags one that never ends', () => {
    expect(tokenRisks(token({ expiresAt: null }), NOW)).toEqual(['never_expires']);
  });

  it('flags one about to end, not one that already has', () => {
    expect(tokenRisks(token({ expiresAt: NOW + 3 * DAY }), NOW)).toEqual(['expiring']);
    expect(tokenRisks(token({ expiresAt: NOW - DAY }), NOW)).toEqual([]);
  });

  it('flags one not used for 90 days, and one never used since long ago', () => {
    expect(tokenRisks(token({ lastUsedAt: NOW - 91 * DAY }), NOW)).toEqual(['unused']);
    expect(tokenRisks(token({ lastUsedAt: null, createdAt: NOW - 100 * DAY }), NOW)).toEqual(['unused']);
    expect(tokenRisks(token({ lastUsedAt: null, createdAt: NOW - DAY }), NOW)).toEqual([]);
  });

  it('flags one that can do all its owner can', () => {
    expect(tokenRisks(token({ policyId: null, permissions: null }), NOW)).toEqual(['unscoped']);
    expect(tokenRisks(token({ policyId: null, permissions: ['alerts.rules.read'] }), NOW)).toEqual([]);
  });

  it('has nothing to say about one that can no longer do anything', () => {
    expect(tokenRisks(token({ revoked: true, expiresAt: null, policyId: null }), NOW)).toEqual([]);
    expect(tokenRisks(token({ expiresAt: NOW - DAY, policyId: null }), NOW)).toEqual([]);
  });
});

describe('countRisks', () => {
  it('counts a token under each thing it has', () => {
    const counts = countRisks([token({ expiresAt: null, policyId: null }), token({ policyId: null })], NOW);
    expect(counts).toEqual({ never_expires: 1, unused: 0, unscoped: 2, expiring: 0 });
  });
});
