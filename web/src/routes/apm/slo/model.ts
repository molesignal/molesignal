import type { TFunction } from 'i18next';

import type { AlertRuleInput } from '@/api/alerts';
import type { ServiceLevelObjective, SloState } from '@/api/apmSlo';
import { apmRuleLabels } from '@/routes/alerts/rule-workbench/apmSource';

import { formatDuration } from '../format';

/** Each state of an objective as a word in its own ink. */
export const SLO_STATE_TEXT: Record<SloState, string> = {
  healthy: 'text-[var(--green-ink)]',
  at_risk: 'text-[var(--yellow-ink)]',
  breached: 'text-[var(--red-ink)]',
  no_data: 'text-tx-3',
};

/** A state as its words weigh in a row: a met objective in the body weight, one at risk or missed in the strong one. */
export const SLO_STATE_WEIGHT: Record<SloState, string> = {
  healthy: 'font-body',
  at_risk: 'font-strong',
  breached: 'font-strong',
  no_data: 'font-body',
};

/** The fill of the budget bar, in the colour of the state. */
export const SLO_STATE_FILL: Record<SloState, string> = {
  healthy: 'var(--green)',
  at_risk: 'var(--yellow)',
  breached: 'var(--red)',
  no_data: 'var(--tx-4)',
};

export const SLO_STATE_ORDER: readonly SloState[] = ['breached', 'at_risk', 'healthy', 'no_data'];

/** A share close to one, with as many decimals as the objective needs to be told apart: 99.95%. */
export function formatSli(value: number | null | undefined): string {
  if (value === null || value === undefined) return '—';
  const percent = value * 100;
  const digits = percent >= 99.9 ? 3 : percent >= 99 ? 2 : 1;
  return `${percent.toFixed(digits).replace(/\.?0+$/, '')}%`;
}

/** The part of the error budget left, which goes below zero once the objective is broken. */
export function formatBudget(value: number | null | undefined): string {
  if (value === null || value === undefined) return '—';
  return `${Math.round(value * 100)}%`;
}

export function formatBurn(value: number | null | undefined): string {
  if (value === null || value === undefined) return '—';
  return `${value >= 10 ? value.toFixed(0) : value.toFixed(1)}×`;
}

/** What makes a request good, as a phrase: "available", or "within 300 ms". */
export function sloTarget(slo: Pick<ServiceLevelObjective, 'kind' | 'latencyThreshold'>, t: TFunction<'apm'>): string {
  return slo.kind === 'latency'
    ? t('slo.target.latency', { threshold: formatDuration(slo.latencyThreshold ?? undefined) })
    : t('slo.target.availability');
}

/**
 * The two burn-rate rules an objective is watched by, as the SRE workbook
 * pairs them: one pages when an hour (and its last five minutes) spends the
 * budget 14.4 times too fast, one warns when six hours (and their last half
 * hour) spend it six times too fast. Both are APM rules on the objective.
 */
export function burnRateRules(slo: ServiceLevelObjective, names: { fast: string; slow: string; description: string }): AlertRuleInput[] {
  const source = {
    namespace: slo.namespace,
    service: slo.service,
    environment: slo.environment,
    transaction: slo.transaction ?? null,
    metric: 'slo_burn_rate' as const,
    sloId: slo.id,
  };
  const labels = { ...apmRuleLabels(source), 'apm.slo': slo.id };
  const rule = (name: string, periodSecs: number, shortSecs: number, threshold: number, severity: 'critical' | 'warning'): AlertRuleInput => ({
    name,
    description: names.description,
    enabled: true,
    kind: 'scheduled',
    query: {
      language: 'sql',
      statement: '',
      periodSecs: periodSecs,
      stream: null,
      apm: { ...source, shortWindowSecs: shortSecs },
    },
    trigger: { operator: 'gt', threshold, forPeriods: 1, silenceSecs: 600 },
    thresholds: [{ severity, operator: 'gt', threshold, forPeriods: 1 }],
    severity: null,
    labels,
    annotations: {},
  });

  return [rule(names.fast, 3_600, 300, 14.4, 'critical'), rule(names.slow, 21_600, 1_800, 6, 'warning')];
}
