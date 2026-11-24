import type { TFunction } from 'i18next';

import type { CorrelationGrade, ServiceCorrelation } from '@/api/apmCorrelation';
import { cn } from '@/shell/lib/cn';

import { CORRELATION_ASPECTS, type CorrelationAspect, tracedShare } from './model';

const DOT: Record<CorrelationGrade, string> = {
  good: 'bg-[var(--green)]',
  partial: 'bg-[var(--yellow)]',
  missing: 'bg-tx-4',
  unknown: 'bg-bd-1',
};

const RESOURCE_ATTRIBUTES = ['environment', 'version', 'instance', 'placement'] as const;

/** What one way of tying says of the service, in a line. */
export function aspectDetail(entry: ServiceCorrelation, aspect: CorrelationAspect, t: TFunction, locale: string): string {
  const number = (value: number) => value.toLocaleString(locale);

  switch (aspect) {
    case 'traceIds': {
      const share = tracedShare(entry);
      if (entry.logs.state === 'unknown') return t('correlation.detail.unknown');
      if (share === null) return t('correlation.detail.no_logs');

      const traced = t('correlation.detail.traced', {
        share: share.toLocaleString(locale, { style: 'percent', maximumFractionDigits: 0 }),
        lines: number(entry.logs.lines),
      });
      return entry.logs.sampled > 0
        ? `${traced} ${t('correlation.detail.matched', { matched: number(entry.logs.matched), sampled: number(entry.logs.sampled) })}`
        : traced;
    }
    case 'resource': {
      const missing = RESOURCE_ATTRIBUTES.filter((attribute) => !entry.resource[attribute]).map((attribute) => t(`correlation.attribute.${attribute}`));
      return missing.length === 0 ? t('correlation.detail.resource_complete') : t('correlation.detail.resource_missing', { attributes: missing.join(t('correlation.and')) });
    }
    case 'metrics':
      return t(`correlation.detail.metrics_${entry.metrics}`);
    case 'changes':
      if (entry.grades.changes === 'unknown') return t('correlation.detail.unknown');
      return t('correlation.detail.changes', { count: entry.changes.count, formatted: number(entry.changes.count) });
  }
}

/**
 * One dot for each way a service ties to other data: its logs to its traces,
 * its spans' resource attributes, its metrics, its reported changes. Green
 * ties well, yellow in part, grey not at all; the faintest could not be told.
 */
export function CorrelationDots({ entry, t, locale }: { entry: ServiceCorrelation | null; t: TFunction; locale: string }) {
  if (!entry) return <span className="text-tx-4">—</span>;

  const lines = CORRELATION_ASPECTS.map(
    (aspect) => `${t(`correlation.aspect.${aspect}`)}: ${t(`correlation.grade.${entry.grades[aspect]}`)}. ${aspectDetail(entry, aspect, t, locale)}`,
  );

  return (
    <span role="img" aria-label={lines.join('\n')} title={lines.join('\n')} data-apm-correlation="" className="inline-flex items-center gap-1">
      {CORRELATION_ASPECTS.map((aspect) => (
        <span key={aspect} aria-hidden data-aspect={aspect} data-grade={entry.grades[aspect]} className={cn('h-2 w-2 rounded-full', DOT[entry.grades[aspect]])} />
      ))}
    </span>
  );
}
