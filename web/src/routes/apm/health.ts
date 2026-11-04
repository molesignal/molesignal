import type { TFunction } from 'i18next';

import type { HealthReason, ServiceHealth } from '@/api/apm';

import { formatDuration, formatRate } from './format';

/** A figure past its line, as a phrase: what it is, and the line it passed. */
export function healthReasonText(reason: HealthReason, t: TFunction<'apm'>): string {
  const format = (value: number) => (reason.metric === 'error_rate' ? formatRate(value) : formatDuration(value));
  return t(`health.reason.${reason.metric}`, { value: format(reason.value), threshold: format(reason.threshold) });
}

/** A service's health with what made it so: "Warning: error rate 5.20%, at or past 2.00%". */
export function describeHealth(health: ServiceHealth, reasons: readonly HealthReason[] | undefined, t: TFunction<'apm'>): string {
  const state = t(`health.${health}`);
  if (!reasons?.length) return state;
  return t('health.with_reasons', { health: state, reasons: reasons.map((reason) => healthReasonText(reason, t)).join(t('health.reason_separator')) });
}
