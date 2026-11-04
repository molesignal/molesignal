import type { ApiToken } from '@/api/apiTokens';

export type TokenRisk = 'never_expires' | 'unused' | 'unscoped' | 'expiring';

export const TOKEN_RISKS: readonly TokenRisk[] = ['never_expires', 'unused', 'unscoped', 'expiring'];

const DAY_MICROS = 86_400_000_000;
/** Not used for this long, a token is likely forgotten and still works. */
const UNUSED_AFTER = 90 * DAY_MICROS;
/** Ending within this long, something that depends on it is about to stop. */
const EXPIRING_WITHIN = 14 * DAY_MICROS;

/**
 * What about a token is worth a second look. A revoked or expired one can do
 * nothing, so it has none. These are prompts for a review, not faults: an intake token is
 * often meant to live for ever.
 */
export function tokenRisks(token: ApiToken, nowMicros: number): TokenRisk[] {
  const expires = token.expiresAt ?? null;
  if (token.revoked || (expires !== null && expires <= nowMicros)) return [];

  const risks: TokenRisk[] = [];

  if (expires === null) risks.push('never_expires');
  else if (expires - nowMicros <= EXPIRING_WITHIN) risks.push('expiring');

  const lastSeen = token.lastUsedAt ?? token.createdAt;
  if (nowMicros - lastSeen >= UNUSED_AFTER) risks.push('unused');

  // Neither a policy nor a list of permissions: it can do all its owner can.
  if (!token.policyId && !token.permissions?.length) risks.push('unscoped');

  return risks;
}

export function countRisks(tokens: readonly ApiToken[], nowMicros: number): Record<TokenRisk, number> {
  const counts: Record<TokenRisk, number> = { never_expires: 0, unused: 0, unscoped: 0, expiring: 0 };

  for (const token of tokens) {
    for (const risk of tokenRisks(token, nowMicros)) counts[risk] += 1;
  }

  return counts;
}
