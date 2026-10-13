import type * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { ApmMeta, RedSummary, TrendPoint } from '@/api/apm';
import { cn } from '@/shell/lib/cn';
import { SpectrumBar } from '@/shell/readout/SpectrumBar';

import { RedReadoutCells } from '../RedReadouts';

export interface ServiceHealthCounts {
  healthy: number;
  warning: number;
  critical: number;
  no_traffic: number;
}

export type HealthVerdict = 'critical' | 'warning' | 'healthy' | 'idle';

/** What the health counts add up to: the worst state any service is in. */
export function healthVerdict(counts: ServiceHealthCounts): HealthVerdict {
  if (counts.critical > 0) return 'critical';
  if (counts.warning > 0) return 'warning';
  if (counts.healthy > 0) return 'healthy';
  return 'idle';
}

const VERDICT_COLOR: Record<HealthVerdict, string> = {
  critical: 'var(--red)',
  warning: 'var(--yellow)',
  healthy: 'var(--green)',
  idle: 'var(--tx-3)',
};

const VERDICT_TEXT: Record<HealthVerdict, string> = {
  critical: 'text-red',
  warning: 'text-yellow',
  healthy: 'text-green',
  idle: 'text-tx-0',
};

/**
 * The opening statement of the APM overview: are the services well, and what
 * are requests, errors and latency doing.
 *
 * One surface instead of a row of number cards. The verdict on the left is the
 * sentence a reader came for, with the spread of service health under it; the
 * RED figures beside it each carry the shape they have had over the window, so
 * "1.1% errors" also says whether that is a spike or the usual.
 */
export function ApmPulse({
  red,
  trend,
  health,
  resolution,
}: {
  red: RedSummary;
  trend: TrendPoint[];
  health: ServiceHealthCounts;
  resolution: ApmMeta['resolution'];
}) {
  const { t } = useTranslation('apm');
  const verdict = healthVerdict(health);
  const total = health.healthy + health.warning + health.critical + health.no_traffic;
  const title =
    verdict === 'critical' || verdict === 'warning'
      ? t(`pulse.verdict.${verdict}`, { count: health[verdict] })
      : t(`pulse.verdict.${verdict}`);

  return (
    <section
      aria-label={t('health.title')}
      data-apm-pulse
      data-verdict={verdict}
      style={{ '--tone': VERDICT_COLOR[verdict] } as React.CSSProperties}
      className="ms-rise grid overflow-hidden rounded-md bg-bd-0 [box-shadow:inset_3px_0_0_var(--tone),var(--shadow-functional-surface)] xl:grid-cols-[minmax(300px,0.8fr)_minmax(0,2.2fr)] xl:gap-px"
    >
      <div
        // The verdict's own colour pools in the corner it is read from.
        style={{
          backgroundImage:
            'radial-gradient(120% 170% at 0% 0%, color-mix(in srgb, var(--tone) 9%, transparent), transparent 62%)',
        }}
        className="min-w-0 bg-[var(--functional-surface)] px-[20px] py-[16px]"
      >
        <div className="type-eyebrow font-sans text-tx-2">{t('pulse.eyebrow')}</div>
        <h2
          className={cn(
            'type-display mt-[6px] text-balance font-sans font-display tracking-normal',
            VERDICT_TEXT[verdict],
          )}
        >
          {title}
        </h2>
        <p className="type-data mt-1 text-pretty text-tx-1">
          {t('pulse.detail', { healthy: health.healthy, total })}
        </p>
        <SpectrumBar
          className="mt-[14px]"
          label={t('pulse.distribution')}
          segments={[
            { key: 'healthy', label: t('health.healthy'), value: health.healthy, color: 'var(--green)' },
            { key: 'warning', label: t('health.warning'), value: health.warning, color: 'var(--yellow)' },
            { key: 'critical', label: t('health.critical'), value: health.critical, color: 'var(--red)' },
            {
              key: 'no_traffic',
              label: t('health.no_traffic'),
              value: health.no_traffic,
              color: 'var(--tx-3)',
            },
          ]}
        />
      </div>

      <div className="grid min-w-0 gap-px border-t border-bd-0 sm:grid-cols-2 xl:grid-cols-4 xl:border-t-0">
        <RedReadoutCells red={red} trend={trend} resolution={resolution} firstIndex={1} />
      </div>
    </section>
  );
}
