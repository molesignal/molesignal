import { describe, expect, it } from 'vitest';

import { BUDGET_EXHAUSTED, budgetRefusal } from './chat';

describe('budgetRefusal', () => {
  it('reads the day-budget refusal into a blocking notice', () => {
    const body = JSON.stringify({
      error: BUDGET_EXHAUSTED,
      code: BUDGET_EXHAUSTED,
      message: 'the organization daily budget is used up',
      usedTokens: 51_200,
      limitTokens: 50_000,
      resetsAt: 1_760_659_200_000_000,
    });

    expect(budgetRefusal(429, body)).toEqual({
      kind: 'budget',
      level: 'exhausted',
      usedTokens: 51_200,
      limitTokens: 50_000,
      resetsAt: 1_760_659_200_000_000,
      blocked: true,
    });
  });

  it('leaves every other failure to the ordinary error path', () => {
    expect(budgetRefusal(500, JSON.stringify({ error: BUDGET_EXHAUSTED }))).toBeNull();
    expect(budgetRefusal(429, JSON.stringify({ error: 'resource_exhausted' }))).toBeNull();
    expect(budgetRefusal(429, 'Too Many Requests')).toBeNull();
  });
});
