import type { useTranslation } from 'react-i18next';

import type { SeverityThreshold } from '@/types/alerting';

/** The annotations of a rule without the legacy `channels` one. */
export function withoutLegacyNotifyAnnotations(
  annotations: Record<string, string>,
): Record<string, string> {
  return Object.fromEntries(
    Object.entries(annotations).filter(([key]) => key !== 'channels'),
  );
}

export function isValidHttpUrl(value: string): boolean {
  try {
    const url = new URL(value);
    return url.protocol === 'http:' || url.protocol === 'https:';
  } catch {
    return false;
  }
}

export function formatNumber(value?: number): string {
  if (value === undefined) return '—';
  if (Math.abs(value) >= 1000) return value.toLocaleString(undefined, { maximumFractionDigits: 2 });
  return value.toLocaleString(undefined, { maximumFractionDigits: 4 });
}

/** Throws the message of the first thing that keeps the rule from being saved. */
export function validateBeforeSave({
  name,
  streamName,
  query,
  thresholds,
  hasConflict,
  t,
}: {
  name: string;
  streamName: string;
  query: string;
  thresholds: SeverityThreshold[];
  hasConflict: boolean;
  t: ReturnType<typeof useTranslation<'alerts'>>['t'];
}) {
  if (!name.trim()) throw new Error(t('workbench.errors.name_required'));
  if (!streamName.trim()) throw new Error(t('workbench.errors.stream_required'));
  if (streamName.trim() === '_audit') throw new Error(t('workbench.errors.stream_audit'));
  if (!query.trim()) throw new Error(t('workbench.errors.query_required'));
  if (thresholds.length === 0 || thresholds.some((band) => !Number.isFinite(band.threshold))) {
    throw new Error(t('workbench.errors.threshold_required'));
  }
  if (hasConflict) throw new Error(t('workbench.errors.threshold_conflict'));
}
