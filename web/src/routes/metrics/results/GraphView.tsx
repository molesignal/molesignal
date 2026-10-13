import { AlertTriangle } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { ChromeButton } from '@/shell/chrome';
import { QueryState } from '@/shell/query/State';
import { TimeSeriesChart } from '@/viz/timeseries/TimeSeriesChart';

import { ExemplarRail } from '../ExemplarRail';
import {
  MetricsNoDataState,
  MetricsQueryErrorState,
} from './RecoveryState';
import { MetricsStartState } from './StartState';
import type { GraphViewProps } from './types';

/**
 * The graph and what stands in for it. The plot takes the height the page has
 * left under the query, so folding the editor away hands that room to the chart.
 */
export function GraphView({
  query,
  series,
  chart,
  exemplars,
  recovery,
  starter,
  onViewRawCounter,
  onInspectMetricType,
}: GraphViewProps) {
  const { t } = useTranslation('metrics');
  const canStack = series.chartSeries.length > 1;
  const effectiveStackMode = canStack ? chart.stackMode : 'none';

  return (
    <div className="flex min-h-0 flex-[1_0_auto] flex-col p-3">
      {query.error ? (
        <MetricsQueryErrorState error={query.error} recovery={recovery} />
      ) : query.pending && !query.result ? (
        <QueryState
          state="loading"
          loadingLabel={t('explore.editor.running_promql')}
        />
      ) : !query.executedPromql && !query.result ? (
        <MetricsStartState starter={starter} />
      ) : series.metricSeries.length === 0 ? (
        <MetricsNoDataState recovery={recovery} />
      ) : (
        <>
          {series.counterRateQuery && series.quality.negativePoints > 0 ? (
            <NegativeRateNotice
              count={series.quality.negativePoints}
              onViewRawCounter={onViewRawCounter}
              onInspectMetricType={onInspectMetricType}
            />
          ) : null}
          <TimeSeriesChart
            height="auto"
            className="flex-[1_0_auto]"
            series={series.chartSeries}
            xDomain={chart.xDomain}
            seriesIdentity={{
              title: t('explore.series.title'),
              countLabel: t('explore.results.series_count', {
                count: series.metricSeries.length,
              }),
              nameLabel: t('explore.series.columns.name'),
              labelCountLabel: (count) => t('explore.series.label_count', { count }),
              expandLabel: (metricName) => t('explore.series.expand_labels', { metric: metricName }),
              collapseLabel: (metricName) => t('explore.series.collapse_labels', { metric: metricName }),
              statLabels: {
                last: t('explore.series.columns.last'),
              },
            }}
            options={{
              drawStyle: chart.drawStyle,
              fillOpacity: 0,
              stackMode: effectiveStackMode,
              connectNulls: true,
              showPoints: 'auto',
              legendMode: 'table',
              legendPlacement: 'bottom',
              legendStats: ['last'],
              leftAxis: {
                ...(series.unit ? { unit: series.unit } : {}),
                ...(series.counterRateQuery ? { softMin: 0 } : {}),
              },
              ...(series.quality.negativePoints > 0
                ? {
                    bands: [{ to: 0, color: 'var(--red-dim)' }],
                    thresholds: [
                      {
                        value: 0,
                        color: 'var(--red-soft)',
                        showLine: true,
                      },
                    ],
                  }
                : {}),
            }}
            {...(chart.timezone ? { timezone: chart.timezone } : {})}
            onRangeSelect={chart.onRangeSelect}
            {...(chart.zoomed ? { onRangeReset: chart.onRangeReset } : {})}
            rangeSelectionAriaLabel={t('explore.chart.zoom_aria')}
          />
          <ExemplarRail
            series={exemplars.series}
            fromMicros={chart.xDomain[0]}
            toMicros={chart.xDomain[1]}
            {...(exemplars.warning ? { warning: exemplars.warning } : {})}
            {...(exemplars.error ? { error: exemplars.error } : {})}
          />
        </>
      )}
    </div>
  );
}

function NegativeRateNotice({
  count,
  onViewRawCounter,
  onInspectMetricType,
}: {
  count: number;
  onViewRawCounter: () => void;
  onInspectMetricType: () => void;
}) {
  const { t } = useTranslation('metrics');
  return (
    <div
      className="mb-3 flex shrink-0 flex-wrap items-start gap-3 rounded-md border border-red/30 bg-red-dim px-3 py-2.5"
      role="alert"
    >
      <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0 text-red" aria-hidden="true" />
      <div className="min-w-[16rem] flex-1">
        <div className="text-sm font-semibold text-tx-0">
          {t('explore.quality.negative_rate_title')}
        </div>
        <div className="mt-0.5 text-xs leading-5 text-tx-2">
          {t('explore.quality.negative_rate_description', { count })}
        </div>
      </div>
      <div className="flex shrink-0 items-center gap-1.5">
        <ChromeButton size="sm" onClick={onViewRawCounter}>
          {t('explore.quality.view_raw')}
        </ChromeButton>
        <ChromeButton size="sm" onClick={onInspectMetricType}>
          {t('explore.quality.inspect_type')}
        </ChromeButton>
      </div>
    </div>
  );
}
