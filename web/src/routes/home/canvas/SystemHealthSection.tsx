import { Activity } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import type * as homeApi from '@/api/home';
import type * as streamsApi from '@/api/streams';
import { summarizeSignalHealth } from '@/investigation/streamHealth';
import { Dot, uiLabelClass, uiLabelStrongClass } from '@/shell/chrome';
import { cn } from '@/shell/lib/cn';
import { QueryState } from '@/shell/query/State';
import type { queryStateFor } from '@/shell/query/State';
import { formatRelativeMicros } from '@/time/relative';
import { TimeSeriesChart } from '@/viz/timeseries/TimeSeriesChart';

import { formatBytes, formatCount, formatEventRate } from './format';
import { CanvasHeaderAction, CanvasSection } from './layout';

export type HomeChartMetric = 'intake' | 'stored' | 'rows';

const STATUS_DOT: Record<
  homeApi.HomeHealthStatus,
  'green' | 'red' | 'yellow' | 'dim'
> = {
  healthy: 'green',
  degraded: 'red',
  delayed: 'yellow',
  no_data: 'dim',
  unknown: 'dim',
};

export function SystemHealthSection({
  overview,
  runtimeOverview,
  state,
  error,
  metric,
  onMetricChange,
  windowLabel,
  onOpenStreams,
}: {
  overview: homeApi.HomeOverview | undefined;
  runtimeOverview: streamsApi.StreamRuntimeOverview | undefined;
  state: ReturnType<typeof queryStateFor>;
  error: unknown;
  metric: HomeChartMetric;
  onMetricChange: (metric: HomeChartMetric) => void;
  windowLabel: string;
  onOpenStreams: () => void;
}) {
  const { t, i18n } = useTranslation('onboarding');
  const metricOptions: Array<{ id: HomeChartMetric; label: string }> = [
    { id: 'intake', label: t('home.health.metrics.intake') },
    { id: 'stored', label: t('home.health.metrics.stored') },
    { id: 'rows', label: t('home.health.metrics.events') },
  ];
  const chartData = (overview?.buckets ?? []).map((bucket) => {
    if (metric === 'intake') return bucket.intake_bytes ?? 0;
    if (metric === 'stored') return bucket.stored_bytes;
    return bucket.rows;
  });
  const chartTotal =
    metric === 'intake'
      ? overview?.intake_bytes
      : metric === 'stored'
        ? overview?.stored_bytes
        : overview?.rows;
  const chartTotalLabel =
    metric === 'rows' ? formatCount(chartTotal) : formatBytes(chartTotal);
  const hasChartData = chartData.some((value) => value > 0);
  const timestamps = (overview?.buckets ?? []).map((bucket) =>
    Math.round((bucket.start_micros + bucket.end_micros) / 2),
  );
  const chartDomain: [number, number] = [
    overview?.window.start_micros ?? 0,
    overview?.window.end_micros ?? 1,
  ];
  const chartValues = chartData.length > 0 ? chartData : [0, 0];
  const chartTimestamps = timestamps.length > 0 ? timestamps : chartDomain;
  const signalHealth = (['logs', 'metrics', 'traces', 'profiles'] as const)
    .map((streamType) => ({
      streamType,
      ...summarizeSignalHealth(runtimeOverview?.streams ?? [], streamType),
    }))
    .filter((signal) => signal.total > 0);

  return (
    <CanvasSection
      className="home-canvas-primary-section"
      title={t('home.health.title')}
      icon={<Activity className="h-4 w-4 text-indigo-soft" />}
      actions={<span className="font-sans text-xs text-tx-2">{windowLabel}</span>}
    >
      {state ? (
        <div className="grid h-full min-h-[268px] place-items-stretch">
          <QueryState state={state} error={error} emptyLabel={t('home.health.empty')} />
        </div>
      ) : (
        <div className="grid h-full min-h-[268px] gap-4 px-4 pb-4 pt-3 lg:grid-cols-[minmax(0,1fr)_minmax(230px,0.34fr)]">
          <div className="flex h-full min-w-0 flex-col">
            <div className="flex flex-wrap items-start justify-between gap-3">
              <div>
                <div className={uiLabelClass}>{t('home.health.total')}</div>
                <div className="mt-1.5 font-sans text-2xl font-display-strong tracking-[-0.02em] text-tx-0">
                  {chartTotalLabel}
                </div>
              </div>
              <div className="flex rounded-md bg-bg-2 p-0.5">
                {metricOptions.map((item) => (
                  <button
                    key={item.id}
                    type="button"
                    disabled={item.id === 'intake' && overview?.intake_bytes == null}
                    aria-pressed={metric === item.id}
                    onClick={() => onMetricChange(item.id)}
                    className={cn(
                      'rounded px-2.5 py-1.5 font-sans text-xs font-strong transition-colors disabled:cursor-not-allowed disabled:opacity-40 focus-visible:outline-none',
                      metric === item.id
                        ? 'bg-bg-4 text-tx-0'
                        : 'text-tx-2 hover:bg-bg-3 hover:text-tx-0 focus-visible:bg-bg-3 focus-visible:text-tx-0',
                    )}
                  >
                    {item.label}
                  </button>
                ))}
              </div>
            </div>
            <div className="mt-3 min-h-[164px] flex-1">
              <TimeSeriesChart
                series={[
                  {
                    name:
                      metricOptions.find((item) => item.id === metric)?.label ?? metric,
                    color: hasChartData
                      ? metric === 'intake'
                        ? 'var(--chart-1)'
                        : metric === 'stored'
                          ? 'var(--chart-2)'
                          : 'var(--chart-7)'
                      : 'transparent',
                    data: chartValues,
                    timestamps: chartTimestamps,
                    unit: metric === 'rows' ? 'events' : 'bytes',
                  },
                ]}
                xDomain={chartDomain}
                height="100%"
                showLegend={false}
                options={{
                  drawStyle: 'bar',
                  compactAxes: true,
                  ...(hasChartData ? {} : { tooltipMode: 'hidden' }),
                }}
              />
            </div>
          </div>

          <div className="min-w-0 pt-4 lg:pl-1 lg:pt-0">
            <div className="flex items-center justify-between">
              <span className={uiLabelStrongClass}>{t('home.health.signals')}</span>
              <CanvasHeaderAction label={t('home.view_all')} onClick={onOpenStreams} />
            </div>
            <div className="mt-1.5 space-y-0.5">
              {signalHealth.map((signal) => (
                <div key={signal.streamType} className="flex min-h-[46px] items-center gap-3 py-2">
                  <Dot tone={STATUS_DOT[signal.status]} />
                  <div className="min-w-0 flex-1">
                    <div className="flex items-center justify-between gap-2">
                      <span className="font-sans text-sm font-strong capitalize text-tx-0">
                        {signal.streamType}
                      </span>
                      <span className="font-sans text-xs text-tx-2">
                        {formatEventRate(
                          signal.rows,
                          runtimeOverview?.window_secs ?? overview?.window?.window_secs ?? 1,
                        )}
                      </span>
                    </div>
                    <div className="mt-0.5 flex items-center justify-between gap-2 font-sans text-xs text-tx-2">
                      <span>{t(`home.status.${signal.status}`)}</span>
                      <span>
                        {formatRelativeMicros(
                          signal.lastReceivedAtMicros,
                          i18n.resolvedLanguage ?? i18n.language,
                          runtimeOverview?.generated_at_micros,
                        )}
                      </span>
                    </div>
                  </div>
                </div>
              ))}
            </div>
          </div>
        </div>
      )}
    </CanvasSection>
  );
}
