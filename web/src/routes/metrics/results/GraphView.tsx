import { AlertTriangle, MousePointer2, RotateCcw } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { ChromeButton } from '@/shell/chrome';
import { QueryState } from '@/shell/query/State';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/shell/ui/select';
import { TimeSeriesChart } from '@/viz/timeseries/TimeSeriesChart';

import { ExemplarRail } from '../ExemplarRail';
import type { MetricsDrawStyle, MetricsStackMode } from '../model';
import type { GraphViewProps } from './types';

export function GraphView({
  query,
  series,
  chart,
  exemplars,
  onViewRawCounter,
  onInspectMetricType,
}: GraphViewProps) {
  const { t } = useTranslation('metrics');
  const canStack = series.chartSeries.length > 1;
  const effectiveStackMode = canStack ? chart.stackMode : 'none';

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="flex min-h-12 shrink-0 flex-wrap items-center gap-3 border-b border-bd-0 px-3 py-2">
        <div className="min-w-[12rem] flex-1">
          <div
            className={`truncate font-sans text-sm font-semibold ${
              query.promql.trim() ? 'text-tx-0' : 'text-tx-3'
            }`}
          >
            {query.promql.trim()
              ? query.chartTitle
              : t('explore.chart.awaiting_query')}
          </div>
          {query.promql.trim() ? (
            <code className="type-micro mt-0.5 block truncate font-mono text-tx-3">
              {query.executedPromql ?? query.promql}
            </code>
          ) : null}
        </div>

        {chart.zoomed ? (
          <ChromeButton
            size="sm"
            onClick={chart.onRangeReset}
            title={t('explore.chart.reset_zoom_hint')}
            data-testid="metrics-reset-zoom"
            className="h-11 sm:h-8"
          >
            <RotateCcw className="h-3.5 w-3.5" aria-hidden="true" />
            {t('explore.chart.reset_zoom')}
          </ChromeButton>
        ) : (
          <span
            className="hidden items-center gap-1.5 font-sans text-xs text-tx-3 xl:inline-flex"
            title={t('explore.chart.zoom_hint')}
          >
            <MousePointer2 className="h-3.5 w-3.5" aria-hidden="true" />
            {t('explore.chart.zoom_hint')}
          </span>
        )}
        <DrawStyleControl
          value={chart.drawStyle}
          onChange={chart.onDrawStyleChange}
        />
        <StackModeControl
          value={effectiveStackMode}
          disabled={!canStack}
          onChange={chart.onStackModeChange}
        />
      </div>

      <div className="flex min-h-0 flex-1 flex-col p-3">
        {query.error ? (
          <QueryState state="error" error={query.error} />
        ) : query.pending && !query.result ? (
          <QueryState
            state="loading"
            loadingLabel={t('explore.editor.running_promql')}
          />
        ) : !query.executedPromql && !query.result ? (
          <QueryState
            state="empty"
            emptyLabel={t('explore.chart.select_prompt')}
            className="min-h-[clamp(320px,44vh,520px)]"
          />
        ) : series.metricSeries.length === 0 ? (
          <QueryState state="empty" emptyLabel={t('explore.chart.empty')} />
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
              height="100%"
              className="min-h-[360px] flex-1"
              series={series.chartSeries}
              xDomain={chart.xDomain}
              options={{
                drawStyle: chart.drawStyle,
                fillOpacity: 0,
                stackMode: effectiveStackMode,
                connectNulls: true,
                showPoints: 'auto',
                legendMode: 'table',
                legendPlacement: 'bottom',
                legendStats: ['last', 'min', 'max', 'mean'],
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
    </div>
  );
}

function StackModeControl({
  value,
  disabled,
  onChange,
}: {
  value: MetricsStackMode;
  disabled: boolean;
  onChange: (mode: MetricsStackMode) => void;
}) {
  const { t } = useTranslation('metrics');
  return (
    <Select
      value={value}
      disabled={disabled}
      onValueChange={(next) => onChange(next as MetricsStackMode)}
    >
      <SelectTrigger
        aria-label={t('explore.chart.stack_mode')}
        title={
          disabled
            ? t('explore.chart.stack_requires_multiple_series')
            : undefined
        }
        className="h-11 w-[8.75rem] border-bd-0 bg-bg-2 px-2 text-base sm:h-8 sm:text-xs"
        data-testid="metrics-stack-mode"
      >
        <span className="flex min-w-0 items-center gap-1.5">
          <span className="shrink-0 text-tx-2">
            {t('explore.chart.stack_mode')}
          </span>
          <SelectValue />
        </span>
      </SelectTrigger>
      <SelectContent align="end">
        <SelectItem value="none" className="text-xs">
          {t('explore.chart.stack_none')}
        </SelectItem>
        <SelectItem value="normal" className="text-xs">
          {t('explore.chart.stack_normal')}
        </SelectItem>
        <SelectItem value="percent" className="text-xs">
          {t('explore.chart.stack_percent')}
        </SelectItem>
      </SelectContent>
    </Select>
  );
}

function DrawStyleControl({
  value,
  onChange,
}: {
  value: MetricsDrawStyle;
  onChange: (style: MetricsDrawStyle) => void;
}) {
  const { t } = useTranslation('metrics');
  return (
    <div
      className="flex h-11 items-center rounded-md border border-bd-0 bg-bg-2 p-0.5 sm:h-8"
      aria-label={t('explore.chart.draw_style')}
    >
      {(
        [
          ['line', t('explore.chart.mode_line')],
          ['bar', t('explore.chart.mode_bar')],
          ['points', t('explore.chart.mode_points')],
        ] as const
      ).map(([id, label]) => (
        <button
          key={id}
          type="button"
          onClick={() => onChange(id)}
          aria-pressed={value === id}
          className={`h-full rounded px-2 font-sans text-xs focus-visible:bg-bg-3 ${
            value === id
              ? 'bg-bg-4 font-semibold text-tx-0'
              : 'text-tx-2 hover:bg-bg-3 hover:text-tx-0'
          }`}
        >
          {label}
        </button>
      ))}
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
      className="mb-3 flex flex-wrap items-start gap-3 rounded-md border border-red/30 bg-red-dim px-3 py-2.5"
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
