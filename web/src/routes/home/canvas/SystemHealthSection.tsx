import { Activity } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import type * as homeApi from '@/api/home';
import type * as streamsApi from '@/api/streams';
import { Dot } from '@/shell/chrome';
import { EmptyIllustration } from '@/shell/EmptyIllustration';
import { EmptyState } from '@/shell/EmptyState';
import { cn } from '@/shell/lib/cn';
import { QueryState } from '@/shell/query/State';
import type { queryStateFor } from '@/shell/query/State';
import { formatRelativeMicros } from '@/time/relative';
import { TimeSeriesChart } from '@/viz/timeseries/TimeSeriesChart';

import { EmptyPlot } from './EmptyPlot';
import {
  formatBytes,
  formatCount,
  formatEventRate,
  splitMetricValue,
} from './format';
import { PlotSkeleton } from './HomeSkeletons';
import { CanvasHeaderAction, CanvasSection } from './layout';
import {
  summarizeFleet,
  toMicros,
  VERDICT_CONDITION,
  VERDICT_TONE,
} from './streamCondition';

export type HomeChartMetric = 'intake' | 'stored' | 'rows';

const SIGNAL_TYPES = ['logs', 'metrics', 'traces', 'profiles'] as const;

function summarizeSignals(runtime: streamsApi.StreamRuntimeOverview | undefined) {
  const streams = runtime?.streams ?? [];
  const now =
    runtime?.generated_at_micros != null
      ? toMicros(runtime.generated_at_micros)
      : Date.now() * 1000;
  return SIGNAL_TYPES.map((type) => {
    const matching = streams.filter((stream) => stream.stream_type === type);
    return {
      type,
      fleet: summarizeFleet(matching, now),
      rows: matching.reduce((total, stream) => total + stream.rows, 0),
    };
  }).filter((signal) => signal.fleet.total > 0);
}

export function SystemHealthSection({
  overview,
  runtimeOverview,
  state,
  error,
  metric,
  onMetricChange,
  windowLabel,
  onOpenStreams,
  onConnect,
  onInvestigate,
  riseIndex,
}: {
  overview: homeApi.HomeOverview | undefined;
  runtimeOverview: streamsApi.StreamRuntimeOverview | undefined;
  state: ReturnType<typeof queryStateFor>;
  error: unknown;
  metric: HomeChartMetric;
  onMetricChange: (metric: HomeChartMetric) => void;
  windowLabel: string;
  onOpenStreams: () => void;
  onConnect: () => void;
  onInvestigate: () => void;
  riseIndex?: number;
}) {
  const { t, i18n } = useTranslation(['onboarding', 'nav']);
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
  const total = splitMetricValue(
    metric === 'rows' ? formatCount(chartTotal) : formatBytes(chartTotal),
  );
  const hasChartData = chartData.some((value) => value > 0);
  const timestamps = (overview?.buckets ?? []).map((bucket) =>
    Math.round((bucket.start_micros + bucket.end_micros) / 2),
  );
  const chartDomain: [number, number] = [
    overview?.window?.start_micros ?? 0,
    overview?.window?.end_micros ?? 1,
  ];
  const signals = summarizeSignals(runtimeOverview);
  const windowSecs =
    runtimeOverview?.window_secs ?? overview?.window?.window_secs ?? 1;
  const generatedAtMicros = runtimeOverview?.generated_at_micros;

  const body = () => {
    if (state === 'loading') return <PlotSkeleton />;
    if (state === 'error') {
      return (
        <div className="grid h-full min-h-[240px] place-items-stretch">
          <QueryState state="error" error={error} />
        </div>
      );
    }
    if (state === 'empty') {
      return (
        <EmptyState
          size="compact"
          icon={Activity}
          illustration={<EmptyIllustration kind="streams" />}
          title={t('home.health.empty')}
          primaryAction={{ label: t('home.connect_action'), onClick: onConnect }}
        />
      );
    }
    return (
      <div className="home-health-body min-h-0 flex-1 px-[20px] pb-[20px] pt-1">
        <div className="flex h-full min-w-0 flex-col">
          <div className="flex flex-wrap items-start justify-between gap-3">
            <div>
              <div className="type-caption text-tx-2">
                {t('home.health.total_window', { window: windowLabel })}
              </div>
              <div className="type-kpi mt-1.5 flex items-baseline gap-1.5 font-sans font-display tracking-[-0.02em] tabular-nums text-tx-0">
                <span>{total.number}</span>
                {total.unit && (
                  <span className="type-data font-medium tracking-normal text-tx-2">
                    {total.unit}
                  </span>
                )}
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
          <div className="mt-3 min-h-[160px] flex-1">
            {hasChartData ? (
              <div className="home-reveal-up h-full">
                <TimeSeriesChart
                  series={[
                    {
                      name:
                        metricOptions.find((item) => item.id === metric)?.label ??
                        metric,
                      color:
                        metric === 'intake'
                          ? 'var(--chart-1)'
                          : metric === 'stored'
                            ? 'var(--chart-2)'
                            : 'var(--chart-7)',
                      data: chartData,
                      timestamps,
                      unit: metric === 'rows' ? 'events' : 'bytes',
                    },
                  ]}
                  xDomain={chartDomain}
                  height="100%"
                  showLegend={false}
                  options={{ drawStyle: 'bar', compactAxes: true }}
                />
              </div>
            ) : (
              <EmptyPlot
                title={t('home.health.no_window_data')}
                hint={t('home.health.no_window_data_hint')}
                actionLabel={t('home.health.check_action')}
                onAction={onInvestigate}
              />
            )}
          </div>
        </div>

        <div className="min-w-0">
          <div className="flex items-center justify-between">
            <span className="type-label font-medium text-tx-2">
              {t('home.health.signals')}
            </span>
            <CanvasHeaderAction label={t('home.view_all')} onClick={onOpenStreams} />
          </div>
          <ul className="mt-1.5">
            {signals.map(({ type, fleet, rows }) => {
              const condition = VERDICT_CONDITION[fleet.verdict];
              return (
                <li key={type} className="flex min-h-[48px] items-center gap-3 py-2">
                  <Dot tone={VERDICT_TONE[fleet.verdict]} />
                  <div className="min-w-0 flex-1">
                    <div className="flex items-center justify-between gap-2">
                      <span className="font-sans text-sm font-strong text-tx-0">
                        {t(`nav:${type}`)}
                      </span>
                      <span className="type-caption tabular-nums text-tx-2">
                        {formatEventRate(rows, windowSecs)}
                      </span>
                    </div>
                    <div className="type-caption mt-0.5 flex items-center justify-between gap-2 text-tx-2">
                      <span>
                        {t(`home.condition.${condition}`)}
                        {/* The worst state, and how many streams are in it: a type with one
                            stale stream and four healthy ones should not read as "all stale". */}
                        {!fleet.uniform && ` · ${fleet.affected}/${fleet.total}`}
                      </span>
                      <span>
                        {formatRelativeMicros(
                          fleet.lastReceivedAtMicros,
                          i18n.resolvedLanguage ?? i18n.language,
                          generatedAtMicros,
                        )}
                      </span>
                    </div>
                  </div>
                </li>
              );
            })}
          </ul>
        </div>
      </div>
    );
  };

  return (
    <CanvasSection
      className="home-canvas-chart-section"
      title={t('home.health.title')}
      icon={<Activity className="h-4 w-4 text-indigo-soft" />}
      {...(riseIndex !== undefined ? { riseIndex } : {})}
    >
      {body()}
    </CanvasSection>
  );
}
