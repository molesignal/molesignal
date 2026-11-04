import type { TFunction } from 'i18next';
import { describe, expect, it } from 'vitest';

import type { ServiceLevelObjective } from '@/api/apmSlo';

import { burnRateRules, formatBudget, formatBurn, formatSli, sloTarget } from './model';

const slo: ServiceLevelObjective = {
  id: 'slo-1',
  name: 'Checkout availability',
  description: '',
  namespace: 'shop',
  service: 'checkout',
  environment: 'prod',
  transaction: { name: 'POST /orders', kind: 'http' },
  kind: 'availability',
  objective: 0.999,
  windowDays: 30,
  createdBy: 'u',
  createdAt: 0,
  updatedBy: 'u',
  updatedAt: 0,
};

describe('service level objectives', () => {
  it('writes shares with the decimals an objective needs', () => {
    expect(formatSli(0.99953)).toBe('99.953%');
    expect(formatSli(0.995)).toBe('99.5%');
    expect(formatSli(0.9)).toBe('90%');
    expect(formatSli(null)).toBe('—');
    expect(formatBudget(-0.4)).toBe('-40%');
    expect(formatBurn(14.4)).toBe('14×');
    expect(formatBurn(2.04)).toBe('2.0×');
  });

  it('says what makes a request good', () => {
    const t = ((key: string, options?: Record<string, unknown>) =>
      `${key}${options?.threshold ? ` ${String(options.threshold)}` : ''}`) as unknown as TFunction<'apm'>;
    expect(sloTarget(slo, t)).toBe('slo.target.availability');
    expect(sloTarget({ kind: 'latency', latencyThreshold: 300_000 }, t)).toBe('slo.target.latency 300 ms');
  });

  it('watches an objective with a fast and a slow burn-rate rule on it', () => {
    const [fast, slow] = burnRateRules(slo, { fast: 'fast', slow: 'slow', description: 'd' });

    expect(fast?.query.apm).toMatchObject({ metric: 'slo_burn_rate', sloId: 'slo-1', shortWindowSecs: 300, transaction: slo.transaction });
    expect(fast?.query.periodSecs).toBe(3_600);
    expect(fast?.thresholds).toEqual([{ severity: 'critical', operator: 'gt', threshold: 14.4, forPeriods: 1 }]);
    expect(slow?.query).toMatchObject({ periodSecs: 21_600, apm: { shortWindowSecs: 1_800 } });
    expect(slow?.thresholds?.[0]).toMatchObject({ severity: 'warning', threshold: 6 });
    expect(fast?.labels).toMatchObject({ 'service.name': 'checkout', 'deployment.environment': 'prod', 'apm.endpoint': 'POST /orders', 'apm.slo': 'slo-1' });
  });
});
