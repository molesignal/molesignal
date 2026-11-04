import { HeartPulse } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import type { HealthTrendPoint, ServiceHealthCounts } from '@/api/apm';
import { cn } from '@/shell/lib/cn';
import { TimeSeriesChart } from '@/viz/timeseries/TimeSeriesChart';

import { HEALTH_ORDER, HEALTH_TEXT, healthSeries } from './model';
import { ApmCard } from '../frame/Page';

/**
 * How the services divide by health now, and how many were in each state
 * over the range: a column of the chart is a slot of the range, its slices
 * the services in each state then.
 */
export function HealthCard({
  counts,
  trend,
  range,
}: {
  counts: ServiceHealthCounts;
  trend: readonly HealthTrendPoint[];
  range: { from: number; to: number };
}) {
  const { t } = useTranslation('apm');

  return (
    <ApmCard
      icon={HeartPulse}
      tone="text-[var(--green-ink)]"
      title={t('landing.health')}
      description={t('landing.health_description')}
      testId="overview-health"
      bodyClassName="px-2 pb-2 pt-1"
      actions={
        <span className="flex items-center gap-3 font-sans text-xs">
          {HEALTH_ORDER.map((health) => (
            <span key={health} data-apm-health-count={health} className={cn('font-strong', counts[health] === 0 ? 'text-tx-3' : HEALTH_TEXT[health])}>
              {t(`health.${health}`)} <span className="tabular-nums">{counts[health]}</span>
            </span>
          ))}
        </span>
      }
    >
      <TimeSeriesChart
        series={healthSeries(trend, (health) => t(`health.${health}`))}
        xDomain={[range.from, range.to]}
        height={180}
        cursorScopeId={null}
        ariaLabel={t('landing.health')}
        showLegend={false}
        options={{
          drawStyle: 'bar',
          stackMode: 'normal',
          showPoints: 'never',
          tooltipMode: 'all',
          compactAxes: true,
          leftAxis: { scale: 'linear', min: 0 },
        }}
      />
    </ApmCard>
  );
}
