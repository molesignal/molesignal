import {
  Activity,
  Filter,
  FilterX,
  ScrollText,
  Waypoints,
  type LucideIcon,
} from 'lucide-react';
import * as React from 'react';
import uPlot from 'uplot';
import 'uplot/dist/uPlot.min.css';

import { useScope } from '@/keyboard/controller';
import { CopyIconButton } from '@/shell/CopyIconButton';
import { cn } from '@/shell/lib/cn';

import {
  buildTimeSeriesAxisScale,
  resolveStackedAxisOptions,
} from './axisModes';
import { buildYAxisSize } from './axisSizing';
import {
  installBrush,
  resolveInteractionRange,
  type TimeRangeInteractionState,
} from './brush';
import {
  colorWithAlpha,
  resolveCanvasColor,
  timeSeriesColors,
  timeSeriesKey,
} from './colors';
import { useCursorSync } from './cursorSync';
import {
  downsampleAlignedData,
  prepareTimeSeriesData,
  selectAlignedDataByX,
  toInputTimestamp,
} from './data';
import {
  formatCompactNumber,
  formatTimeSeriesAxisTimestamp,
  formatTimeSeriesTimestamp,
  formatTimeSeriesValue,
} from './formatters';
import {
  TimeSeriesLegend,
  type TimeSeriesLegendSelectionMode,
  type TimeSeriesLegendSeries,
} from './legend/TimeSeriesLegend';
import { useThemePalette } from './themeAdapter';
import {
  DEFAULT_TIME_SERIES_OPTIONS,
  type TimeSeriesAxisOptions,
  type TimeSeriesChartOptions,
  type TimeSeriesChartOptionsInput,
  type TimeSeriesRange,
  type TimeSeriesSeries,
  type TimeSeriesSignalContext,
} from './types';

type ResolvedSeries = TimeSeriesLegendSeries;

interface TooltipItem {
  id: string;
  name: string;
  value: number;
  color: string;
  labels: Record<string, string>;
  unit?: string;
}

interface TooltipState {
  timestamp: number;
  inputTimestamp: number;
  left: number;
  top: number;
  items: TooltipItem[];
}

export interface TimeSeriesChartProps {
  title?: string;
  description?: string;
  series: ReadonlyArray<TimeSeriesSeries>;
  height?: React.CSSProperties['height'];
  xDomain?: readonly [number, number];
  options?: TimeSeriesChartOptionsInput;
  /** Overrides options.legendMode for compact integrations. */
  showLegend?: boolean;
  /** Keeps dense embedded charts readable without changing global legend sizing. */
  legendDensity?: 'default' | 'compact';
  loading?: boolean;
  error?: Error | null;
  loadingLabel?: string;
  emptyLabel?: string;
  errorLabel?: string;
  ariaLabel?: string;
  rangeSelectionAriaLabel?: string;
  timezone?: string;
  cursorScopeId?: string | null | undefined;
  focusedSeriesId?: string | null;
  className?: string;
  onRangeSelect?: (range: TimeSeriesRange) => void;
  onRangeReset?: () => void;
  onPan?: (deltaInInputUnits: number) => void;
  onSeriesFilter?: (
    labels: Record<string, string>,
    mode: 'include' | 'exclude',
  ) => void;
  onOpenLogs?: (context: TimeSeriesSignalContext) => void;
  onOpenMetrics?: (context: TimeSeriesSignalContext) => void;
  onOpenTraces?: (context: TimeSeriesSignalContext) => void;
}

/**
 * Unified Canvas time-series surface used by exploration, dashboards and
 * compact product charts. uPlot owns coordinate calculation and Canvas
 * painting; React owns tooltip, legend, actions and query-range changes.
 */
export const TimeSeriesChart = React.memo(function TimeSeriesChart({
  title,
  description,
  series,
  height = 220,
  xDomain,
  options: optionsInput,
  showLegend,
  legendDensity = 'default',
  loading = false,
  error = null,
  loadingLabel = 'Loading chart…',
  emptyLabel = 'No data in this time range',
  errorLabel = 'Unable to render chart',
  ariaLabel,
  rangeSelectionAriaLabel,
  timezone,
  cursorScopeId = 'main',
  focusedSeriesId = null,
  className,
  onRangeSelect,
  onRangeReset,
  onPan,
  onSeriesFilter,
  onOpenLogs,
  onOpenMetrics,
  onOpenTraces,
}: TimeSeriesChartProps) {
  const rootRef = React.useRef<HTMLDivElement | null>(null);
  const plotHostRef = React.useRef<HTMLDivElement | null>(null);
  const plotRef = React.useRef<uPlot | null>(null);
  const preparedRef = React.useRef<ReturnType<typeof prepareTimeSeriesData> | null>(null);
  const externalXDomainRef = React.useRef<readonly [number, number] | undefined>(
    undefined,
  );
  const rangeInteractionRef = React.useRef<TimeRangeInteractionState | null>(
    null,
  );
  const resolvedSeriesRef = React.useRef<ResolvedSeries[]>([]);
  const tooltipRef = React.useRef<TooltipState | null>(null);
  const interactionHandlersRef = React.useRef({
    onRangeSelect,
    onRangeReset,
    onPan,
  });
  interactionHandlersRef.current = { onRangeSelect, onRangeReset, onPan };
  const pinnedRef = React.useRef(false);
  const animationFrameRef = React.useRef<number | null>(null);
  const [tooltip, setTooltip] = React.useState<TooltipState | null>(null);
  const [pinned, setPinned] = React.useState(false);
  const [hiddenIds, setHiddenIds] = React.useState<Set<string>>(() => new Set());
  const [legendFocusedSeriesId, setLegendFocusedSeriesId] = React.useState<
    string | null
  >(null);
  const [fontsReady, setFontsReady] = React.useState(
    () => typeof document === 'undefined' || !document.fonts,
  );
  const resolveVisibleXDomain = React.useCallback(():
    | [number, number]
    | undefined => {
    const externalRange = externalXDomainRef.current;
    const interaction = rangeInteractionRef.current;
    if (!externalRange) {
      return interaction
        ? [interaction.from, interaction.to]
        : undefined;
    }
    const resolved = resolveInteractionRange(externalRange, interaction);
    rangeInteractionRef.current = resolved.interaction;
    return resolved.range;
  }, []);
  const options = React.useMemo(() => {
    const resolved = resolveOptions(optionsInput, showLegend);
    const leftUnit = sharedSeriesUnit(series, 'left');
    const rightUnit = sharedSeriesUnit(series, 'right');
    const leftAxis = resolveStackedAxisOptions(
      {
        ...resolved.leftAxis,
        ...(!resolved.leftAxis.unit && leftUnit ? { unit: leftUnit } : {}),
      },
      resolved.stackMode,
    );
    const baseRightAxis: TimeSeriesAxisOptions | undefined = resolved.rightAxis
      ? {
          ...resolved.rightAxis,
          ...(!resolved.rightAxis.unit && rightUnit ? { unit: rightUnit } : {}),
        }
      : rightUnit
        ? { ...resolved.leftAxis, unit: rightUnit }
        : undefined;
    const rightAxis = baseRightAxis
      ? resolveStackedAxisOptions(baseRightAxis, resolved.stackMode)
      : undefined;
    return {
      ...resolved,
      leftAxis,
      ...(rightAxis ? { rightAxis } : {}),
    };
  }, [optionsInput, series, showLegend]);
  const resolvedSeries = React.useMemo(() => {
    const ids = series.map(timeSeriesKey);
    const automaticColors = timeSeriesColors(ids);
    return series.map((item, index) => {
        const id = ids[index]!;
        return {
          ...item,
          id,
          color: item.color ?? automaticColors[index]!,
        };
      });
  }, [series]);
  const prepared = React.useMemo(
    () => prepareTimeSeriesData(resolvedSeries, xDomain, options.stackMode),
    [options.stackMode, resolvedSeries, xDomain],
  );
  externalXDomainRef.current = prepared.xDomain;
  resolvedSeriesRef.current = resolvedSeries;
  const hasData = prepared.rawData
    .slice(1)
    .some((column) =>
      Array.from(column).some(
        (value) => typeof value === 'number' && Number.isFinite(value),
      ),
    );

  const { version: themeVersion } = useThemePalette();
  const cursorSyncEnabled = cursorScopeId !== null;
  const { onCursorMove } = useCursorSync(
    plotRef,
    cursorScopeId ?? 'main',
    cursorSyncEnabled,
  );
  const onCursorMoveRef = React.useRef(onCursorMove);
  onCursorMoveRef.current = onCursorMove;
  useScope('chart-brush', Boolean(onRangeSelect || onPan));

  React.useEffect(() => {
    if (typeof document === 'undefined' || !document.fonts) return;
    let cancelled = false;
    void document.fonts.ready.then(() => {
      if (!cancelled) setFontsReady(true);
    });
    return () => {
      cancelled = true;
    };
  }, []);

  React.useEffect(() => {
    const available = new Set(resolvedSeries.map((item) => item.id));
    setHiddenIds((current) => {
      const next = new Set([...current].filter((id) => available.has(id)));
      return setsEqual(current, next) ? current : next;
    });
  }, [resolvedSeries]);

  const structureKey = React.useMemo(
    () =>
      JSON.stringify({
        series: resolvedSeries.map((item) => ({
          id: item.id,
          color: item.color,
          axis: item.axis ?? 'left',
          width: item.lineWidth,
          dash: item.dash,
        })),
        height,
        drawStyle: options.drawStyle,
        interpolation: options.interpolation,
        lineWidth: options.lineWidth,
        fillOpacity: options.fillOpacity,
        showPoints: options.showPoints,
        stackMode: options.stackMode,
        connectNulls: options.connectNulls,
        showXAxis: options.showXAxis,
        showYAxis: options.showYAxis,
        compactAxes: options.compactAxes,
        leftAxis: options.leftAxis,
        rightAxis: options.rightAxis,
        thresholds: options.thresholds,
        bands: options.bands,
        annotations: options.annotations,
        timezone,
        hasData,
        hasXDomain: Boolean(prepared.xDomain),
        rangeSelection: Boolean(onRangeSelect),
        rangePan: Boolean(onRangeSelect || onPan),
      }),
    [
      hasData,
      height,
      onPan,
      onRangeSelect,
      options,
      prepared.xDomain,
      resolvedSeries,
      timezone,
    ],
  );

  React.useLayoutEffect(() => {
    const host = plotHostRef.current;
    if (
      !host ||
      !fontsReady ||
      prepared.pointCount === 0 ||
      !canvasIsAvailable()
    ) {
      return;
    }
    const width = Math.max(host.clientWidth || 600, 40);
    const fallbackHeight = typeof height === 'number' ? height : 220;
    const plotHeight = Math.max(host.clientHeight || fallbackHeight, 48);
    const target = Math.max(Math.round(width * 3), 96);
    const renderData =
      prepared.pointCount > target * 2
        ? downsampleAlignedData(prepared.data, target)
        : prepared.data;
    preparedRef.current = {
      ...prepared,
      data: renderData,
      rawData:
        renderData === prepared.data
          ? prepared.rawData
          : selectAlignedDataByX(prepared.rawData, Array.from(renderData[0] ?? [])),
      pointCount: renderData[0]?.length ?? 0,
    };
    const palette = getChartPalette();
    const seriesOptions: uPlot.Series[] = [
      { label: 'time' },
      ...resolvedSeries.map((item, index) =>
        buildSeriesOptions(item, index, resolvedSeries.length, options, palette),
      ),
    ];
    const hasRightAxis = resolvedSeries.some((item) => item.axis === 'right');
    const scales: uPlot.Options['scales'] = {
      x: {
        time: prepared.hasTime,
        ...(prepared.xDomain
          ? {
              range: () =>
                resolveVisibleXDomain() ?? [
                  prepared.xDomain![0],
                  prepared.xDomain![1],
                ],
            }
          : {}),
      },
      y: buildTimeSeriesAxisScale(options.leftAxis),
      ...(hasRightAxis
        ? {
            y2: buildTimeSeriesAxisScale(
              options.rightAxis ?? options.leftAxis,
            ),
          }
        : {}),
    };

    const plot = new uPlot(
      {
        width,
        height: plotHeight,
        legend: { show: false },
        scales,
        series: seriesOptions,
        axes: buildAxes(prepared.hasTime, options, palette, hasRightAxis, timezone),
        cursor: {
          drag: {
            x: Boolean(interactionHandlersRef.current.onRangeSelect),
            y: false,
            setScale: false,
          },
          focus: { prox: 12 },
          points: { size: 7, width: 2 },
        },
        hooks: {
          setCursor: [
            (instance) => {
              const idx = instance.cursor.idx;
              const left = instance.cursor.left ?? -1;
              if (idx === null || idx === undefined || left < 0) {
                if (!pinnedRef.current) setTooltip(null);
                return;
              }
              const timestamp = Number(instance.data[0]?.[idx]);
              if (!Number.isFinite(timestamp)) return;
              if (prepared.hasTime) onCursorMoveRef.current(timestamp);
              if (pinnedRef.current || options.tooltipMode === 'hidden') return;
              scheduleTooltipUpdate(
                animationFrameRef,
                () =>
                  buildTooltipState(
                    instance,
                    idx,
                    preparedRef.current!,
                    resolvedSeriesRef.current,
                    options,
                    rootRef.current,
                  ),
                setTooltip,
              );
            },
          ],
          drawClear: [
            (instance) => drawBands(instance, options, palette),
          ],
          draw: [
            (instance) => {
              if (options.drawStyle === 'bar') {
                drawBars(instance, resolvedSeries, options);
              }
              drawThresholdsAndAnnotations(
                instance,
                prepared.inputTimestampScale,
                options,
                palette,
              );
            },
          ],
        },
      },
      renderData,
      host,
    );
    plotRef.current = plot;
    host.querySelectorAll('canvas').forEach((canvas) => {
      canvas.setAttribute('aria-hidden', 'true');
    });

    const teardownBrush = installBrush(plot, {
      ...(interactionHandlersRef.current.onRangeSelect
        ? {
            onRangeSelect: (range) =>
              interactionHandlersRef.current.onRangeSelect?.({
                from: toInputTimestamp(range.from, prepared.inputTimestampScale),
                to: toInputTimestamp(range.to, prepared.inputTimestampScale),
              }),
          }
        : {}),
      ...(interactionHandlersRef.current.onPan
        ? {
            onPan: (delta) =>
              interactionHandlersRef.current.onPan?.(
                toInputTimestamp(delta, prepared.inputTimestampScale),
              ),
          }
        : {}),
      onRangeInteractionChange: (state) => {
        rangeInteractionRef.current = state;
      },
      onClick: () => {
        if (!tooltipRef.current) return;
        setPinned((current) => {
          const next = !current;
          pinnedRef.current = next;
          return next;
        });
      },
      onRangeReset: () => interactionHandlersRef.current.onRangeReset?.(),
    });
    const observer = new ResizeObserver(() => {
      if (!plotHostRef.current || !plotRef.current) return;
      const nextWidth = Math.max(plotHostRef.current.clientWidth, 40);
      const nextHeight = Math.max(plotHostRef.current.clientHeight, 48);
      plotRef.current.setSize({ width: nextWidth, height: nextHeight });
    });
    observer.observe(host);

    return () => {
      observer.disconnect();
      teardownBrush();
      plot.destroy();
      plotRef.current = null;
    };
    // `structureKey` deliberately captures every option that requires a fresh
    // uPlot series/scale structure; data-only changes use setData below.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [fontsReady, structureKey, themeVersion]);

  React.useEffect(() => {
    const plot = plotRef.current;
    const host = plotHostRef.current;
    if (!plot || !host) return;
    const target = Math.max(Math.round(Math.max(host.clientWidth, 40) * 3), 96);
    const renderData =
      prepared.pointCount > target * 2
        ? downsampleAlignedData(prepared.data, target)
        : prepared.data;
    preparedRef.current = {
      ...prepared,
      data: renderData,
      rawData:
        renderData === prepared.data
          ? prepared.rawData
          : selectAlignedDataByX(prepared.rawData, Array.from(renderData[0] ?? [])),
      pointCount: renderData[0]?.length ?? 0,
    };
    plot.setData(renderData, true);
    const visibleXDomain = resolveVisibleXDomain();
    if (visibleXDomain) {
      plot.setScale('x', {
        min: visibleXDomain[0],
        max: visibleXDomain[1],
      });
    }
  }, [prepared, resolveVisibleXDomain]);

  React.useEffect(() => {
    tooltipRef.current = tooltip;
  }, [tooltip]);

  const effectiveFocusedSeriesId =
    focusedSeriesId ?? legendFocusedSeriesId;

  React.useEffect(() => {
    const plot = plotRef.current;
    if (!plot) return;
    resolvedSeries.forEach((item, index) => {
      const hidden = hiddenIds.has(item.id);
      const dimmed = Boolean(
        effectiveFocusedSeriesId &&
          effectiveFocusedSeriesId !== item.id,
      );
      const plotSeries = plot.series[index + 1];
      if (plotSeries) plotSeries.alpha = dimmed ? 0.2 : 1;
      plot.setSeries(index + 1, {
        show: !hidden,
      });
    });
    plot.redraw(false);
  }, [effectiveFocusedSeriesId, hiddenIds, resolvedSeries]);

  React.useEffect(() => {
    if (!pinned) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return;
      pinnedRef.current = false;
      setPinned(false);
      setTooltip(null);
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [pinned]);

  React.useEffect(
    () => () => {
      if (animationFrameRef.current !== null) {
        cancelAnimationFrame(animationFrameRef.current);
      }
    },
    [],
  );

  const legendVisible = options.legendMode !== 'hidden' && resolvedSeries.length > 0;
  const showResolvedLegend = legendVisible && !loading && !error && hasData;

  return (
    <div
      ref={rootRef}
      className={cn(
        'relative flex min-h-0 min-w-0 flex-col overflow-hidden font-sans text-tx-1',
        className,
      )}
      style={{ height }}
      data-testid="time-series-chart"
      data-renderer="uplot-canvas"
      data-stack-mode={options.stackMode}
      data-cursor-sync={cursorSyncEnabled ? 'shared_crosshair' : 'off'}
      data-pinned={pinned ? 'true' : 'false'}
    >
      {(title || description) && (
        <div className="mb-2 flex min-w-0 shrink-0 items-start justify-between gap-3">
          <div className="min-w-0">
            {title && <div className="truncate text-sm font-strong text-tx-0">{title}</div>}
            {description && <div className="mt-0.5 truncate text-xs text-tx-3">{description}</div>}
          </div>
        </div>
      )}
      <div
        className={cn(
          'flex min-h-0 min-w-0 flex-1',
          showResolvedLegend && options.legendPlacement === 'right'
            ? 'flex-row'
            : 'flex-col',
        )}
        data-legend-placement={options.legendPlacement}
        data-testid="time-series-content"
      >
        {loading ? (
          <ChartState label={loadingLabel} />
        ) : error ? (
          <ChartState label={error.message || errorLabel} tone="error" />
        ) : !hasData ? (
          <ChartState label={emptyLabel} />
        ) : (
          <div
            ref={plotHostRef}
            data-testid="time-series-plot"
            role="img"
            aria-label={ariaLabel ?? rangeSelectionAriaLabel ?? title ?? 'Time series chart'}
            tabIndex={0}
            className={cn(
              'min-h-0 min-w-0 flex-1 overflow-hidden rounded-sm outline-none',
              '[&_.u-wrap]:font-sans [&_.u-over]:cursor-crosshair [&_.u-over.zoom-drag]:!cursor-zoom-in',
            )}
          />
        )}
        {showResolvedLegend && (
          <TimeSeriesLegend
            series={resolvedSeries}
            mode={options.legendMode}
            placement={options.legendPlacement}
            stats={options.legendStats}
            density={legendDensity}
            hiddenIds={hiddenIds}
            focusedSeriesId={effectiveFocusedSeriesId}
            onFocusSeries={setLegendFocusedSeriesId}
            onSelect={(id, mode) =>
              setHiddenIds((current) =>
                updateLegendSelection(
                  current,
                  resolvedSeries,
                  id,
                  mode,
                ),
              )
            }
          />
        )}
      </div>
      {tooltip && (
        <TimeSeriesTooltip
          state={tooltip}
          pinned={pinned}
          onUnpin={() => {
            pinnedRef.current = false;
            setPinned(false);
            setTooltip(null);
          }}
          {...(onSeriesFilter ? { onSeriesFilter } : {})}
          {...(onOpenLogs ? { onOpenLogs } : {})}
          {...(onOpenMetrics ? { onOpenMetrics } : {})}
          {...(onOpenTraces ? { onOpenTraces } : {})}
          {...(timezone ? { timezone } : {})}
        />
      )}
      <div className="sr-only" aria-live="polite">
        {tooltip
          ? `${formatTimeSeriesTimestamp(tooltip.timestamp, true, timezone)}. ${tooltip.items
              .map((item) => `${item.name}: ${formatTimeSeriesValue(item.value, item.unit)}`)
              .join('. ')}`
          : ''}
      </div>
    </div>
  );
});

export interface TimeSeriesSparklineProps {
  data: ReadonlyArray<number | null>;
  timestamps?: ReadonlyArray<number>;
  color?: string;
  fill?: boolean;
  height?: number;
  ariaLabel?: string;
  bands?: TimeSeriesChartOptions['bands'];
  thresholds?: TimeSeriesChartOptions['thresholds'];
  min?: number;
  max?: number;
  className?: string;
}

/** Compact form of the same uPlot renderer; it intentionally has no axes or tooltip. */
export function TimeSeriesSparkline({
  data,
  timestamps,
  color,
  fill = true,
  height = 40,
  ariaLabel,
  bands = [],
  thresholds = [],
  min,
  max,
  className,
}: TimeSeriesSparklineProps) {
  return (
    <TimeSeriesChart
      series={[
        {
          id: ariaLabel ?? 'sparkline',
          name: ariaLabel ?? 'trend',
          data,
          ...(timestamps ? { timestamps } : {}),
          ...(color ? { color } : {}),
        },
      ]}
      height={height}
      ariaLabel={ariaLabel ?? 'Trend'}
      showLegend={false}
      {...(className ? { className } : {})}
      options={{
        drawStyle: fill ? 'area' : 'line',
        fillOpacity: fill ? 0.18 : 0,
        showPoints: 'never',
        tooltipMode: 'hidden',
        showXAxis: false,
        showYAxis: false,
        leftAxis: {
          ...(min !== undefined ? { min } : {}),
          ...(max !== undefined ? { max } : {}),
        },
        bands,
        thresholds,
      }}
    />
  );
}

function resolveOptions(
  input: TimeSeriesChartOptionsInput | undefined,
  showLegend: boolean | undefined,
): TimeSeriesChartOptions {
  const {
    leftAxis: leftAxisInput,
    rightAxis: rightAxisInput,
    ...rest
  } = input ?? {};
  const leftAxis = {
    ...DEFAULT_TIME_SERIES_OPTIONS.leftAxis,
    ...leftAxisInput,
  };
  const rightAxis = rightAxisInput
    ? {
        ...DEFAULT_TIME_SERIES_OPTIONS.leftAxis,
        ...rightAxisInput,
      }
    : undefined;
  return {
    ...DEFAULT_TIME_SERIES_OPTIONS,
    ...rest,
    leftAxis,
    ...(rightAxis ? { rightAxis } : {}),
    legendMode:
      showLegend === false
        ? 'hidden'
        : showLegend === true && input?.legendMode === undefined
          ? DEFAULT_TIME_SERIES_OPTIONS.legendMode
          : input?.legendMode ?? DEFAULT_TIME_SERIES_OPTIONS.legendMode,
    thresholds: input?.thresholds ?? [],
    bands: input?.bands ?? [],
    annotations: input?.annotations ?? [],
    legendStats: input?.legendStats ?? DEFAULT_TIME_SERIES_OPTIONS.legendStats,
  };
}

function sharedSeriesUnit(
  series: ReadonlyArray<TimeSeriesSeries>,
  axis: 'left' | 'right',
): string | undefined {
  const units = new Set(
    series
      .filter((item) => (item.axis ?? 'left') === axis)
      .map((item) => item.unit?.trim())
      .filter((unit): unit is string => Boolean(unit)),
  );
  return units.size === 1 ? [...units][0] : undefined;
}

function buildSeriesOptions(
  series: ResolvedSeries,
  index: number,
  seriesCount: number,
  options: TimeSeriesChartOptions,
  palette: ChartPalette,
): uPlot.Series {
  const stroke = resolveCanvasColor(series.color, palette.accent);
  const drawPoints =
    options.drawStyle === 'points'
      ? true
      : options.showPoints === 'always'
        ? true
        : options.showPoints === 'never'
          ? false
          : (plot: uPlot, _seriesIndex: number, from: number, to: number) =>
              (plot.bbox.width / uPlot.pxRatio) / Math.max(to - from, 1) >= 9;
  let paths: uPlot.Series.PathBuilder | undefined;
  if (options.drawStyle === 'bar') {
    paths = () => null;
  } else if (options.drawStyle === 'points') {
    paths = uPlot.paths.points?.();
  } else if (options.interpolation !== 'linear') {
    paths = uPlot.paths.stepped?.({
      align: options.interpolation === 'stepBefore' ? -1 : 1,
    });
  }
  const fillOpacity =
    options.drawStyle === 'area'
      ? Math.max(options.fillOpacity, seriesCount === 1 ? 0.16 : 0.08)
      : options.fillOpacity;
  return {
    label: series.name,
    scale: series.axis === 'right' ? 'y2' : 'y',
    stroke,
    width: options.drawStyle === 'points' ? 0 : series.lineWidth ?? options.lineWidth,
    ...(paths ? { paths } : {}),
    ...(fillOpacity > 0 ? { fill: colorWithAlpha(stroke, fillOpacity) } : {}),
    spanGaps: options.connectNulls,
    points: {
      show: drawPoints,
      size: options.drawStyle === 'points' ? 6 : 5,
      width: 1.5,
      stroke,
      fill: palette.surface,
    },
    ...(series.dash ? { dash: Array.from(series.dash) } : {}),
    ...(options.stackMode !== 'none' ? { fillTo: 0 } : {}),
    show: true,
    alpha: 1,
    class: `ms-series-${index}`,
  };
}

function buildAxes(
  hasTime: boolean,
  options: TimeSeriesChartOptions,
  palette: ChartPalette,
  hasRightAxis: boolean,
  timezone: string | undefined,
): uPlot.Axis[] {
  // Canvas does not resolve CSS custom properties inside `context.font`.
  // Keep the family explicit so uPlot does not silently fall back to 10px.
  const axisFont = `600 ${
    options.compactAxes ? 11 : 12
  }px "Alibaba PuHuiTi 3.0", "PingFang SC", "Microsoft YaHei", ui-sans-serif, system-ui, sans-serif`;
  const axisLabelFont =
    '600 11px "Alibaba PuHuiTi 3.0", "PingFang SC", "Microsoft YaHei", ui-sans-serif, system-ui, sans-serif';
  const common: Partial<uPlot.Axis> = {
    stroke: palette.muted,
    font: axisFont,
    labelFont: axisLabelFont,
    gap: options.compactAxes ? 5 : 7,
    ticks: {
      stroke: palette.border,
      width: 1,
      size: options.compactAxes ? 4 : 5,
    },
  };
  const yAxisSize = buildYAxisSize(axisFont, options.compactAxes);
  const xAxis: uPlot.Axis = {
    ...common,
    show: options.showXAxis,
    scale: 'x',
    size: options.compactAxes ? 32 : 36,
    space: options.compactAxes ? 72 : 88,
    grid: { show: false, stroke: palette.border, width: 1 },
    values: (plot, splits) => {
      if (!hasTime) return splits.map((value) => formatCompactNumber(value));
      const min = plot.scales.x?.min ?? splits[0] ?? 0;
      const max = plot.scales.x?.max ?? splits.at(-1) ?? min;
      const span = max - min;
      return splits.map((value) => {
        if (
          span > 0 &&
          (Math.abs(value - min) <= span * 0.0035 ||
            Math.abs(max - value) <= span * 0.0035)
        ) {
          return '';
        }
        return formatTimeSeriesAxisTimestamp(value, span, timezone);
      });
    },
  };
  const leftAxis: uPlot.Axis = {
    ...common,
    show: options.showYAxis,
    scale: 'y',
    side: 3,
    size: yAxisSize,
    grid: {
      show: options.leftAxis.showGrid ?? true,
      stroke: palette.grid,
      width: 1,
      dash: [2, 4],
    },
    values: (_plot, splits) =>
      splits.map((value) => formatTimeSeriesValue(value, options.leftAxis.unit)),
    ...(options.leftAxis.label ? { label: options.leftAxis.label } : {}),
  };
  if (!hasRightAxis) return [xAxis, leftAxis];
  const rightOptions = options.rightAxis ?? options.leftAxis;
  return [
    xAxis,
    leftAxis,
    {
      ...common,
      show: options.showYAxis,
      scale: 'y2',
      side: 1,
      size: yAxisSize,
      grid: { show: false },
      values: (_plot, splits) =>
        splits.map((value) => formatTimeSeriesValue(value, rightOptions.unit)),
      ...(rightOptions.label ? { label: rightOptions.label } : {}),
    },
  ];
}

function buildTooltipState(
  plot: uPlot,
  idx: number,
  prepared: ReturnType<typeof prepareTimeSeriesData>,
  series: ReadonlyArray<ResolvedSeries>,
  options: TimeSeriesChartOptions,
  root: HTMLDivElement | null,
): TooltipState | null {
  const timestamp = Number(prepared.rawData[0]?.[idx]);
  if (!Number.isFinite(timestamp)) return null;
  let items = series
    .map((item, seriesIndex): TooltipItem | null => {
      if (plot.series[seriesIndex + 1]?.show === false) return null;
      const value = prepared.rawData[seriesIndex + 1]?.[idx];
      if (typeof value !== 'number' || !Number.isFinite(value)) return null;
      if (options.tooltipHideZeros && value === 0) return null;
      return {
        id: item.id,
        name: item.name,
        value,
        color: item.color,
        labels: { ...(item.labels ?? {}) },
        ...(item.unit ? { unit: item.unit } : {}),
      };
    })
    .filter((item): item is TooltipItem => item !== null);

  if (options.tooltipMode === 'single' && items.length > 1) {
    const cursorTop = plot.cursor.top ?? 0;
    items = [
      items.reduce((closest, item) => {
        const itemIndex = series.findIndex((candidate) => candidate.id === item.id);
        const plottedValue = plot.data[itemIndex + 1]?.[idx];
        const closestIndex = series.findIndex((candidate) => candidate.id === closest.id);
        const closestValue = plot.data[closestIndex + 1]?.[idx];
        const itemDistance =
          typeof plottedValue === 'number'
            ? Math.abs(plot.valToPos(plottedValue, series[itemIndex]?.axis === 'right' ? 'y2' : 'y') - cursorTop)
            : Number.POSITIVE_INFINITY;
        const closestDistance =
          typeof closestValue === 'number'
            ? Math.abs(plot.valToPos(closestValue, series[closestIndex]?.axis === 'right' ? 'y2' : 'y') - cursorTop)
            : Number.POSITIVE_INFINITY;
        return itemDistance < closestDistance ? item : closest;
      }),
    ];
  }
  if (options.tooltipSort === 'asc') items.sort((left, right) => left.value - right.value);
  if (options.tooltipSort === 'desc') items.sort((left, right) => right.value - left.value);
  items = items.slice(0, Math.max(1, options.tooltipMaxItems));
  if (items.length === 0) return null;

  const hostLeft = plot.root.offsetLeft;
  const hostTop = plot.root.offsetTop;
  const desiredLeft = hostLeft + plot.over.offsetLeft + (plot.cursor.left ?? 0) + 14;
  const desiredTop = hostTop + plot.over.offsetTop + Math.max(8, (plot.cursor.top ?? 0) - 20);
  const rootWidth = root?.clientWidth ?? plot.width;
  return {
    timestamp,
    inputTimestamp: toInputTimestamp(timestamp, prepared.inputTimestampScale),
    left: Math.max(8, Math.min(desiredLeft, Math.max(8, rootWidth - 294))),
    top: Math.max(8, desiredTop),
    items,
  };
}

function scheduleTooltipUpdate(
  frameRef: React.MutableRefObject<number | null>,
  build: () => TooltipState | null,
  setTooltip: React.Dispatch<React.SetStateAction<TooltipState | null>>,
): void {
  if (frameRef.current !== null) cancelAnimationFrame(frameRef.current);
  frameRef.current = requestAnimationFrame(() => {
    frameRef.current = null;
    setTooltip(build());
  });
}

function TimeSeriesTooltip({
  state,
  pinned,
  onUnpin,
  onSeriesFilter,
  onOpenLogs,
  onOpenMetrics,
  onOpenTraces,
  timezone,
}: {
  state: TooltipState;
  pinned: boolean;
  onUnpin: () => void;
  onSeriesFilter?: TimeSeriesChartProps['onSeriesFilter'];
  onOpenLogs?: TimeSeriesChartProps['onOpenLogs'];
  onOpenMetrics?: TimeSeriesChartProps['onOpenMetrics'];
  onOpenTraces?: TimeSeriesChartProps['onOpenTraces'];
  timezone?: string;
}) {
  const primary = state.items[0]!;
  const context = contextForTooltip(state, primary);
  const hasActions = Boolean(
    onSeriesFilter || onOpenLogs || onOpenMetrics || onOpenTraces,
  );
  return (
    <div
      role="tooltip"
      data-testid="time-series-tooltip"
      className={cn(
        'absolute z-30 w-[286px] overflow-hidden rounded-md border border-border bg-surface text-foreground shadow-popup',
        pinned ? 'pointer-events-auto' : 'pointer-events-none',
      )}
      style={{ left: state.left, top: state.top }}
    >
      <div className="flex items-center justify-between gap-2 border-b border-bd-0 px-3 py-2">
        <span className="truncate font-sans text-xs font-semibold tabular-nums text-tx-0">
          {formatTimeSeriesTimestamp(state.timestamp, true, timezone)}
        </span>
        {pinned && (
          <button
            type="button"
            onClick={onUnpin}
            className="rounded px-1.5 py-0.5 text-xs text-tx-3 hover:bg-bg-2 hover:text-tx-0 focus-visible:bg-bg-2 focus-visible:text-tx-0"
          >
            Esc
          </button>
        )}
      </div>
      <div className="max-h-64 overflow-y-auto px-3 py-2">
        {state.items.map((item) => (
          <div
            key={item.id}
            className="grid grid-cols-[10px_minmax(0,1fr)_auto] items-center gap-2 py-1"
          >
            <span className="h-2 w-2 rounded-full" style={{ background: item.color }} />
            <span className="truncate text-xs font-semibold text-foreground" title={item.name}>
              {item.name}
            </span>
            <span className="font-sans text-xs font-semibold tabular-nums text-tx-0">
              {formatTimeSeriesValue(item.value, item.unit)}
            </span>
          </div>
        ))}
      </div>
      {pinned && hasActions && (
        <div className="grid grid-cols-3 gap-1 border-t border-bd-0 bg-bg-1 p-1.5">
          {onSeriesFilter && (
            <>
              <TooltipAction
                icon={Filter}
                label="Include"
                onClick={() => onSeriesFilter(primary.labels, 'include')}
              />
              <TooltipAction
                icon={FilterX}
                label="Exclude"
                onClick={() => onSeriesFilter(primary.labels, 'exclude')}
              />
            </>
          )}
          {onOpenMetrics && (
            <TooltipAction icon={Activity} label="Metrics" onClick={() => onOpenMetrics(context)} />
          )}
          {onOpenLogs && (
            <TooltipAction icon={ScrollText} label="Logs" onClick={() => onOpenLogs(context)} />
          )}
          {onOpenTraces && (
            <TooltipAction icon={Waypoints} label="Traces" onClick={() => onOpenTraces(context)} />
          )}
          <CopyIconButton
            label="Copy"
            onClick={() => {
              void navigator.clipboard?.writeText(
                `${primary.name} ${formatTimeSeriesValue(primary.value, primary.unit)}`,
              );
            }}
          />
        </div>
      )}
    </div>
  );
}

function TooltipAction({
  icon: Icon,
  label,
  onClick,
}: {
  icon: LucideIcon;
  label: string;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="inline-flex min-h-8 items-center justify-center gap-1 rounded px-1.5 text-xs text-tx-2 hover:bg-bg-3 hover:text-tx-0 focus-visible:bg-bg-3 focus-visible:text-tx-0"
    >
      <Icon className="h-3 w-3" />
      {label}
    </button>
  );
}

function contextForTooltip(
  state: TooltipState,
  item: TooltipItem,
): TimeSeriesSignalContext {
  const labels = item.labels;
  return {
    timestamp: state.inputTimestamp,
    labels,
    ...(labels.service ? { serviceName: labels.service } : {}),
    ...(labels.trace_id ? { traceId: labels.trace_id } : {}),
    ...(labels.span_id ? { spanId: labels.span_id } : {}),
  };
}

function drawBands(
  plot: uPlot,
  options: TimeSeriesChartOptions,
  palette: ChartPalette,
): void {
  if (options.bands.length === 0) return;
  const context = plot.ctx;
  context.save();
  context.beginPath();
  context.rect(plot.bbox.left, plot.bbox.top, plot.bbox.width, plot.bbox.height);
  context.clip();
  for (const band of options.bands) {
    const topValue = band.to ?? plot.scales.y?.max;
    const bottomValue = band.from ?? plot.scales.y?.min;
    if (topValue === undefined || bottomValue === undefined) continue;
    const top = plot.valToPos(topValue, 'y', true);
    const bottom = plot.valToPos(bottomValue, 'y', true);
    context.fillStyle = resolveCanvasColor(band.color, palette.grid);
    context.fillRect(
      plot.bbox.left,
      Math.min(top, bottom),
      plot.bbox.width,
      Math.abs(bottom - top),
    );
  }
  context.restore();
}

function drawBars(
  plot: uPlot,
  series: ReadonlyArray<ResolvedSeries>,
  options: TimeSeriesChartOptions,
): void {
  const xs = plot.data[0] ?? [];
  if (xs.length === 0) return;
  const context = plot.ctx;
  const ratio = uPlot.pxRatio;
  let minGap = plot.bbox.width;
  for (let index = 1; index < xs.length; index += 1) {
    const left = plot.valToPos(xs[index - 1]!, 'x', true);
    const right = plot.valToPos(xs[index]!, 'x', true);
    minGap = Math.min(minGap, Math.abs(right - left));
  }
  const groupWidth = Math.max(1 * ratio, Math.min(minGap * 0.72, 48 * ratio));
  const visibleCount = Math.max(
    1,
    series.filter((_, index) => plot.series[index + 1]?.show !== false).length,
  );
  const barWidth =
    options.stackMode === 'none'
      ? Math.max(1 * ratio, groupWidth / visibleCount - 1 * ratio)
      : groupWidth;
  const baseline = plot.valToPos(0, 'y', true);
  let visibleSeriesIndex = 0;

  context.save();
  context.beginPath();
  context.rect(plot.bbox.left, plot.bbox.top, plot.bbox.width, plot.bbox.height);
  context.clip();
  series.forEach((item, seriesIndex) => {
    const plotSeries = plot.series[seriesIndex + 1];
    if (plotSeries?.show === false) return;
    const color = resolveCanvasColor(item.color);
    context.fillStyle = color;
    context.globalAlpha = plotSeries?.alpha ?? 0.82;
    const values = plot.data[seriesIndex + 1] ?? [];
    for (let index = 0; index < xs.length; index += 1) {
      const value = values[index];
      if (typeof value !== 'number' || !Number.isFinite(value)) continue;
      const center = plot.valToPos(xs[index]!, 'x', true);
      if (options.stackMode === 'none') {
        const left =
          center -
          groupWidth / 2 +
          visibleSeriesIndex * (groupWidth / visibleCount) +
          0.5 * ratio;
        const valuePosition = plot.valToPos(value, item.axis === 'right' ? 'y2' : 'y', true);
        context.fillRect(
          left,
          Math.min(valuePosition, baseline),
          barWidth,
          Math.max(1 * ratio, Math.abs(baseline - valuePosition)),
        );
      } else {
        const previous = seriesIndex === 0 ? 0 : Number(plot.data[seriesIndex]?.[index] ?? 0);
        const top = plot.valToPos(value, 'y', true);
        const bottom = plot.valToPos(previous, 'y', true);
        context.fillRect(
          center - barWidth / 2,
          Math.min(top, bottom),
          barWidth,
          Math.max(1 * ratio, Math.abs(bottom - top)),
        );
      }
    }
    visibleSeriesIndex += 1;
  });
  context.restore();
}

function drawThresholdsAndAnnotations(
  plot: uPlot,
  timestampScale: number,
  options: TimeSeriesChartOptions,
  palette: ChartPalette,
): void {
  if (options.thresholds.length === 0 && options.annotations.length === 0) return;
  const context = plot.ctx;
  const ratio = uPlot.pxRatio;
  context.save();
  context.beginPath();
  context.rect(plot.bbox.left, plot.bbox.top, plot.bbox.width, plot.bbox.height);
  context.clip();
  context.font = `${12 * ratio}px "Alibaba PuHuiTi 3.0", "PingFang SC", "Microsoft YaHei", ui-sans-serif, system-ui, sans-serif`;
  context.textBaseline = 'bottom';

  for (const threshold of options.thresholds) {
    if (threshold.showLine === false) continue;
    const y = plot.valToPos(threshold.value, 'y', true);
    const color = resolveCanvasColor(threshold.color ?? 'var(--red)', palette.red);
    context.strokeStyle = color;
    context.lineWidth = ratio;
    context.setLineDash([4 * ratio, 4 * ratio]);
    context.beginPath();
    context.moveTo(plot.bbox.left, y);
    context.lineTo(plot.bbox.left + plot.bbox.width, y);
    context.stroke();
    if (threshold.label) {
      context.fillStyle = color;
      context.fillText(
        `${threshold.label} ${formatCompactNumber(threshold.value)}`,
        plot.bbox.left + plot.bbox.width - context.measureText(`${threshold.label} ${formatCompactNumber(threshold.value)}`).width - 4 * ratio,
        y - 3 * ratio,
      );
    }
  }

  for (const annotation of options.annotations) {
    const start = annotation.timestamp * timestampScale;
    const end = annotation.endTimestamp === undefined ? undefined : annotation.endTimestamp * timestampScale;
    const left = plot.valToPos(start, 'x', true);
    const color = resolveCanvasColor(annotation.color ?? 'var(--purple)', palette.accent);
    if (end !== undefined) {
      const right = plot.valToPos(end, 'x', true);
      context.fillStyle = colorWithAlpha(color, 0.12);
      context.fillRect(
        Math.min(left, right),
        plot.bbox.top,
        Math.abs(right - left),
        plot.bbox.height,
      );
    }
    context.strokeStyle = color;
    context.lineWidth = ratio;
    context.setLineDash([3 * ratio, 4 * ratio]);
    context.beginPath();
    context.moveTo(left, plot.bbox.top);
    context.lineTo(left, plot.bbox.top + plot.bbox.height);
    context.stroke();
  }
  context.restore();
}

function ChartState({
  label,
  tone = 'muted',
}: {
  label: string;
  tone?: 'muted' | 'error';
}) {
  return (
    <div
      className={cn(
        'grid min-h-0 flex-1 place-items-center rounded-md border border-dashed border-bd-1 bg-bg-2/40 px-4 text-center text-xs',
        tone === 'error' ? 'text-red-soft' : 'text-tx-3',
      )}
    >
      {label}
    </div>
  );
}

interface ChartPalette {
  surface: string;
  muted: string;
  border: string;
  grid: string;
  accent: string;
  red: string;
}

function getChartPalette(): ChartPalette {
  return {
    surface: resolveCanvasColor('var(--bg-1)', '#111827'),
    muted: resolveCanvasColor('var(--tx-3)', '#778197'),
    border: resolveCanvasColor('var(--bd-1)', '#d6d9e0'),
    grid: colorWithAlpha(resolveCanvasColor('var(--tx-3)', '#778197'), 0.15),
    accent: resolveCanvasColor('var(--indigo)', '#5969d8'),
    red: resolveCanvasColor('var(--red)', '#d64b45'),
  };
}

function canvasIsAvailable(): boolean {
  if (typeof document === 'undefined' || typeof navigator === 'undefined') return false;
  // jsdom intentionally has no Canvas implementation. Skipping construction
  // keeps component tests deterministic while real browsers always take the
  // uPlot path.
  return !navigator.userAgent.toLowerCase().includes('jsdom');
}

function setsEqual(left: ReadonlySet<string>, right: ReadonlySet<string>): boolean {
  return left.size === right.size && [...left].every((item) => right.has(item));
}

function updateLegendSelection(
  current: ReadonlySet<string>,
  series: ReadonlyArray<ResolvedSeries>,
  id: string,
  mode: TimeSeriesLegendSelectionMode,
): Set<string> {
  if (mode === 'append') {
    const next = new Set(current);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    return next;
  }

  const visible = series.filter((item) => !current.has(item.id));
  const alreadyIsolated = visible.length === 1 && visible[0]?.id === id;
  return alreadyIsolated
    ? new Set()
    : new Set(series.filter((item) => item.id !== id).map((item) => item.id));
}
