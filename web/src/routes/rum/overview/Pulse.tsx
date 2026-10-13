import type * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { ExperienceGrade } from '@/api/rum';
import type { ExperienceBucket, OverviewMetrics } from '@/api/rum/overview';
import { cn } from '@/shell/lib/cn';
import { Readout, type ReadoutTone } from '@/shell/readout/Readout';

import { formatDurationMs, formatShare } from '../_helpers';
import { type VitalKey, VITAL_THRESHOLDS, vitalGrade, vitalScale } from './vitals';

const GRADE_TEXT: Record<ExperienceGrade, string> = {
  good: 'text-green',
  needs_improvement: 'text-yellow',
  poor: 'text-red',
  unknown: 'text-tx-3',
};

const GRADE_MARK: Record<ExperienceGrade, string> = {
  good: 'bg-green',
  needs_improvement: 'bg-yellow',
  poor: 'bg-red',
  unknown: 'bg-tx-3',
};

const CELL = 'bg-[var(--functional-surface)] px-[20px] py-[16px]';

const VITALS: readonly VitalKey[] = ['lcp', 'inp', 'cls'];

function formatVital(key: VitalKey, value: number): string {
  if (!(value > 0)) return '—';
  return key === 'cls' ? value.toFixed(3) : formatDurationMs(value);
}

function formatThreshold(key: VitalKey, value: number): string {
  return key === 'cls' ? String(value) : formatDurationMs(value);
}

function errorFreeTone(rate: number): ReadoutTone {
  if (rate >= 0.99) return 'good';
  if (rate >= 0.95) return 'warn';
  return 'danger';
}

/**
 * The opening statement of the RUM overview: how many people, how many of
 * their sessions went without an error, and how the three Core Web Vitals
 * stand against their thresholds.
 *
 * One surface instead of six number cards. Each vital is drawn on its own
 * scale, so "2.3 s" is read as "inside good, close to the line" without the
 * reader knowing the thresholds by heart.
 */
export function RumPulse({
  metrics,
  trend,
  comparison,
}: {
  metrics: OverviewMetrics;
  /** Sessions per bucket over the window; absent until the insights arrive. */
  trend?: readonly ExperienceBucket[] | undefined;
  /** How each figure compares with the period before, when that is asked for. */
  comparison?: Partial<Record<keyof OverviewMetrics, string>> | undefined;
}) {
  const { t } = useTranslation('rum');

  return (
    <section
      aria-label={t('overview.pulse_aria')}
      data-rum-pulse
      className="ms-rise grid gap-px overflow-hidden rounded-md bg-bd-0 [box-shadow:var(--shadow-functional-surface)] xl:grid-cols-[minmax(0,1fr)_minmax(0,1.2fr)]"
    >
      <div className="grid min-w-0 gap-px sm:grid-cols-3">
        <Readout
          className={CELL}
          label={t('overview.kpi.active_users')}
          value={metrics.users.toLocaleString('en-US')}
          detail={
            comparison?.users ??
            (metrics.users > 0
              ? t('overview.sessions_per_user', {
                  value: (metrics.sessions / metrics.users).toFixed(1),
                })
              : undefined)
          }
        />
        <Readout
          className={CELL}
          riseIndex={1}
          label={t('overview.kpi.sessions')}
          value={metrics.sessions.toLocaleString('en-US')}
          trend={
            trend
              ? {
                  values: trend.map((bucket) => bucket.good + bucket.needs + bucket.poor),
                  variant: 'bars',
                }
              : undefined
          }
          detail={comparison?.sessions}
        />
        <Readout
          className={CELL}
          riseIndex={2}
          label={t('overview.kpi.error_free_sessions')}
          value={formatShare(metrics.errorFreeRate)}
          tone={errorFreeTone(metrics.errorFreeRate)}
          detail={
            comparison?.errorFreeRate ??
            t('overview.sessions_with_errors', {
              count: Math.round(metrics.sessions * (1 - metrics.errorFreeRate)).toLocaleString('en-US'),
            })
          }
        />
      </div>
      <div className="grid min-w-0 gap-px sm:grid-cols-3">
        {VITALS.map((key, index) => (
          <VitalReadout
            key={key}
            vital={key}
            value={metrics[`${key}P75`]}
            detail={comparison?.[`${key}P75`]}
            riseIndex={index + 3}
          />
        ))}
      </div>
    </section>
  );
}

function VitalReadout({
  vital,
  value,
  detail,
  riseIndex,
}: {
  vital: VitalKey;
  value: number;
  detail?: string | undefined;
  riseIndex: number;
}) {
  const { t } = useTranslation('rum');
  const grade = vitalGrade(vital, value);
  const scale = vitalScale(vital, value);
  const thresholds = VITAL_THRESHOLDS[vital];

  return (
    <div
      data-rum-vital={vital}
      data-grade={grade}
      style={{ '--rise-index': riseIndex } as React.CSSProperties}
      className={cn('ms-rise flex min-w-0 flex-col', CELL)}
    >
      <div className="flex min-w-0 items-baseline justify-between gap-2">
        <span className="type-label truncate font-medium text-tx-2">
          {t(`overview.kpi.${vital}`)}
        </span>
        <span className={cn('type-caption shrink-0 font-strong', GRADE_TEXT[grade])}>
          {t(`experience.${grade}`)}
        </span>
      </div>
      <div className="type-kpi mt-1.5 truncate font-sans font-display tracking-[-0.02em] tabular-nums text-tx-0">
        {formatVital(vital, value)}
      </div>

      {/* The value on its own scale: good, needs improvement, poor. */}
      <div
        role="img"
        aria-label={t('overview.vital_scale_aria', {
          vital: t(`overview.vitals.${vital}`),
          grade: t(`experience.${grade}`),
        })}
        className="relative mt-[14px] h-[6px]"
      >
        <div className="flex h-full w-full gap-[2px] overflow-hidden rounded-full">
          <span
            className={cn('rounded-full bg-green', grade === 'good' ? 'opacity-90' : 'opacity-25')}
            style={{ width: `${scale.bands.good}%` }}
          />
          <span
            className={cn(
              'rounded-full bg-yellow',
              grade === 'needs_improvement' ? 'opacity-90' : 'opacity-25',
            )}
            style={{ width: `${scale.bands.needsImprovement}%` }}
          />
          <span
            className={cn('rounded-full bg-red', grade === 'poor' ? 'opacity-90' : 'opacity-25')}
            style={{ width: `${scale.bands.poor}%` }}
          />
        </div>
        {scale.position !== null && (
          <span
            data-rum-vital-mark
            className={cn(
              'ms-wipe-dot absolute top-1/2 h-[14px] w-[4px] -translate-x-1/2 -translate-y-1/2 rounded-full [box-shadow:0_0_0_2px_var(--functional-surface)]',
              GRADE_MARK[grade],
            )}
            style={{ left: `${scale.position}%` }}
          />
        )}
      </div>

      <div className="type-caption mt-[10px] truncate text-tx-2">
        {detail ??
          t('overview.vital_thresholds', {
            good: formatThreshold(vital, thresholds.good),
            poor: formatThreshold(vital, thresholds.poor),
          })}
      </div>
    </div>
  );
}
