import { Activity } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { RumAggregate } from '@/api/rum';
import { TimeSeriesChart } from '@/viz/timeseries/TimeSeriesChart';

import { type Metric, rescaleOf, thresholdsOf } from './model';
import { chartSeries } from '../explorer/viz/answer';
import type { RumVizBars } from '../explorer/viz/spec';
import { CardState, OverviewCard } from '../overview/OverviewCard';

type Read = { isPending: boolean; isError: boolean; error: unknown; refetch: () => unknown; data: RumAggregate | undefined };

/**
 * The 75th percentile of a level over the time in view, one point to a bar, with a dashed line where
 * it turns from good to needs improvement and one where it turns poor: the grade of a stretch is read
 * against them. Dragging across the chart narrows the time range, where the page allows it.
 */
export function TrendCard({
  query,
  bars,
  metric,
  language,
  onRangeSelect,
  testId = 'performance-trend',
  height = 200,
}: {
  query: Read;
  bars: RumVizBars;
  metric: Metric;
  language: string;
  onRangeSelect?: ((range: { from: number; to: number }) => void) | undefined;
  testId?: string;
  height?: number;
}) {
  const { t } = useTranslation('rum');
  const name = t(`performance.metrics.${metric.key}`);
  const rescale = React.useMemo(() => rescaleOf(metric), [metric]);
  const thresholds = React.useMemo(
    () => thresholdsOf(metric, { good: t('performance.trend.good'), poor: t('performance.trend.poor') }),
    [metric, t],
  );

  const series = React.useMemo(
    () =>
      query.data
        ? chartSeries(query.data, {
            bars,
            measures: [{ fn: 'p75', field: metric.field }],
            groups: [],
            language,
            none: '',
            measureLabel: () => t('overview.vitals.label', { name }),
            rescale,
          })
        : [],
    [bars, language, metric.field, name, query.data, rescale, t],
  );
  const drawn = series.some((line) => line.data.some((value) => value !== null));

  return (
    <OverviewCard
      icon={Activity}
      tone="text-[var(--chart-1)]"
      title={t('performance.trend.title', { name })}
      description={t('performance.trend.description')}
      testId={testId}
    >
      <CardState query={query} empty={!drawn} emptyTitle={t('performance.trend.empty')}>
        <div className="px-3 pb-3 pt-2">
          <TimeSeriesChart
            height={height}
            series={series}
            xDomain={[bars.frame.start, bars.frame.end]}
            ariaLabel={t('performance.trend.aria', { name })}
            cursorScopeId="rum-performance"
            showLegend={false}
            {...(onRangeSelect && { onRangeSelect })}
            options={{
              drawStyle: 'line',
              showPoints: 'auto',
              legendMode: 'hidden',
              tooltipMode: 'all',
              // The scale reaches past the poor line, so that both lines and their words are in view.
              leftAxis: {
                scale: 'linear',
                softMin: 0,
                softMax: Math.max(...thresholds.map((line) => line.value)) * 1.12,
                ...(rescale ? { unit: rescale.unit } : {}),
              },
              thresholds,
            }}
          />
        </div>
      </CardState>
    </OverviewCard>
  );
}
