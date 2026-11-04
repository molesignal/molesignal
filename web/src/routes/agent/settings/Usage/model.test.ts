import { describe, expect, it } from 'vitest';

import type { DailyUsageRow } from '@/api/agent/usage';

import {
  budgetInput,
  limitChoice,
  statusDayStart,
  usageByDay,
  usedPercent,
} from './model';

const DAY = 86_400_000_000;

function row(
  day: number,
  origin: DailyUsageRow['origin'],
  tokens: number,
  cost = 0,
  unpriced = 0,
  currency: string | null = cost > 0 ? 'USD' : null,
): DailyUsageRow {
  return {
    dayStart: day * DAY,
    origin,
    calls: 1,
    unpricedCalls: unpriced,
    promptTokens: tokens,
    completionTokens: 0,
    cost,
    currency,
  };
}

describe('Mole Agent usage', () => {
  it('reads the share of the limit, and none without a limit', () => {
    expect(usedPercent({ usedTokens: 41_000, limitTokens: 50_000 })).toBe(82);
    expect(usedPercent({ usedTokens: 60_000, limitTokens: 50_000 })).toBe(120);
    expect(usedPercent({ usedTokens: 10, limitTokens: 0 })).toBeNull();
  });

  it('dates a status to the UTC day before it resets', () => {
    expect(statusDayStart({ resetsAt: 20_378 * DAY })).toBe(20_377 * DAY);
  });

  it('groups rows into days, newest first, split by origin', () => {
    const days = usageByDay([
      row(10, 'chat', 100, 0.5),
      row(11, 'chat', 40),
      row(10, 'rca', 60, 0.25),
      row(10, 'llm_eval', 5),
    ]);

    expect(days.map((day) => day.dayStart)).toEqual([11 * DAY, 10 * DAY]);
    expect(days[1]?.tokens).toBe(165);
    expect(days[1]?.calls).toBe(3);
    expect(days[1]?.costs.USD).toBeCloseTo(0.75);
    expect(days[1]?.byOrigin.rca.tokens).toBe(60);
    expect(days[1]?.byOrigin.inspection.tokens).toBe(0);
  });

  it('keeps count of the calls a day\'s cost leaves out', () => {
    const days = usageByDay([row(10, 'chat', 100, 0.5), row(10, 'llm_eval', 5, 0, 1)]);

    expect(days[0]?.unpricedCalls).toBe(1);
    expect(days[0]?.byOrigin.llm_eval.unpricedCalls).toBe(1);
    expect(days[0]?.byOrigin.chat.unpricedCalls).toBe(0);
    expect(days[0]?.costs.USD).toBeCloseTo(0.5);
  });

  it('turns the limit form into what the backend stores', () => {
    expect(limitChoice(null)).toBe('inherit');
    expect(limitChoice(0)).toBe('unlimited');
    expect(limitChoice(50_000)).toBe('custom');

    expect(budgetInput('inherit', '', 'block')).toEqual({ dailyTokenLimit: null, interactiveMode: 'block' });
    expect(budgetInput('unlimited', '', 'warn')).toEqual({ dailyTokenLimit: 0, interactiveMode: 'warn' });
    expect(budgetInput('custom', ' 200000 ', 'block')).toEqual({
      dailyTokenLimit: 200_000,
      interactiveMode: 'block',
    });
    expect(budgetInput('custom', '', 'block')).toBeNull();
    expect(budgetInput('custom', '1.5', 'block')).toBeNull();
    expect(budgetInput('custom', '-3', 'block')).toBeNull();
  });

  it('keeps costs in different currencies apart', () => {
    const [day] = usageByDay([
      row(10, 'chat', 100, 0.5, 0, 'USD'),
      row(10, 'chat', 50, 3.2, 0, 'CNY'),
      row(10, 'rca', 10, 0.25, 0, 'USD'),
      row(10, 'chat', 20, 0, 1, null),
    ]);

    expect(day?.costs).toEqual({ USD: 0.75, CNY: 3.2 });
    expect(day?.byOrigin.chat.costs).toEqual({ USD: 0.5, CNY: 3.2 });
    expect(day?.unpricedCalls).toBe(1);
    expect(day?.tokens).toBe(180);
  });
});
