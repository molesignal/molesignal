import type { TFunction } from 'i18next';

import { isCountingKind, type CountingKind, type CountingParts } from './model';

type Translate = TFunction<'pipelines'>;

/**
 * What a counting step does, for the line under its title: the metric it
 * makes, or how much its limit lets through. Empty for any other step.
 */
export function countingSummary(step: CountingParts & { kind?: string | undefined }, t: Translate): string {
  if (!isCountingKind(step.kind)) return '';

  const kind: CountingKind = step.kind;
  const fields = (list: readonly string[] | undefined) =>
    (list ?? []).map((field) => field.trim()).filter(Boolean).join(', ');

  if (kind === 'generate_metric') {
    const name = step.metric?.name.trim() ?? '';
    const by = fields(step.metric?.groupBy);

    return [name, by && t('counting.summary_by', { fields: by })].filter(Boolean).join(' · ');
  }

  if (kind === 'quota') {
    const quota = step.quota;
    if (!quota?.limit.trim()) return '';

    const limit = t('counting.summary_quota', { limit: quota.limit.trim(), unit: t(`counting.units.${quota.unit}`) });
    const by = fields(quota.partitionBy);

    return [limit, by && t('counting.summary_by', { fields: by })].filter(Boolean).join(' · ');
  }

  const throttle = step.throttle;
  if (!throttle?.limit.trim()) return '';

  const rate = t('counting.summary_throttle', { limit: throttle.limit.trim(), window: throttle.windowSecs.trim() });
  const by = fields(throttle.groupBy);

  return [rate, by && t('counting.summary_by', { fields: by })].filter(Boolean).join(' · ');
}
