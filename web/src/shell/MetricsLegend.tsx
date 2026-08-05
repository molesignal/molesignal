import * as React from 'react';

import { seriesStats, type MetricSeries } from '@/lib/metricsSeries';
import { cn } from '@/shell/lib/cn';
import { detectSignalTypeForLabel, SignalReference } from '@/shell/SignalReference';
import { formatTimeSeriesValue } from '@/viz/timeseries/formatters';

/**
 * Series legend shared by Metrics and dashboard TimeSeriesChart surfaces.
 *
 * Per series, render one row:
 *   ● service="foo" · host="node-3"        last 142.3
 *
 * Each `key="value"` pair whose key resolves to a SignalReferenceType
 * (`service` / `host` / `trace_id` / ...) becomes a HoverCard launcher with
 * 2–3 cross-signal jumps. Other label keys render as plain text so the
 * user still sees the dimension but doesn't get a no-op HoverCard.
 *
 * If more than `maxRowsBeforeCollapse` series exist, collapses to that many
 * with a "Show all N series" toggle. The series order is taken from
 * `rowsToSeries` (first-appearance), so colours stay stable.
 */

export interface MetricsLegendProps {
  series: MetricSeries[];
  colors: string[];
  /** Optional pre-rendered legend labels, e.g. Grafana `legendFormat`. */
  displayNames?: string[] | undefined;
  /** Full labels shown on hover when displayNames are intentionally compact. */
  displayTitles?: string[] | undefined;
  /** Metric identifier rendered before the Grafana-style `{label="value"}` set. */
  metricName?: string | undefined;
  metricQuery?: string | undefined;
  /** Optional handler for hover cross-highlight; the chart can dim other series. */
  onHoverIndex?: ((idx: number | null) => void) | undefined;
  /** Index → visible toggle, click-to-hide; index here matches `series`. */
  hiddenIndexes?: Set<number>;
  onToggleIndex?: ((idx: number) => void) | undefined;
  onSoloIndex?: ((idx: number) => void) | undefined;
  maxRowsBeforeCollapse?: number;
  /** Row list for dense detail views; table adds Current / Avg / Max columns. */
  variant?: 'list' | 'inline' | 'table';
  showLastValue?: boolean;
  unit?: string | undefined;
  labels?: {
    name: string;
    current: string;
    average: string;
    max: string;
    collapse: string;
    showAll: (count: number) => string;
  };
  className?: string | undefined;
}

const DEFAULT_MAX_ROWS = 6;

export function MetricsLegend({
  series,
  colors,
  displayNames,
  displayTitles,
  metricName,
  metricQuery,
  onHoverIndex,
  hiddenIndexes,
  onToggleIndex,
  onSoloIndex,
  maxRowsBeforeCollapse = DEFAULT_MAX_ROWS,
  variant = 'list',
  showLastValue = true,
  unit,
  labels = {
    name: 'Name',
    current: 'Current',
    average: 'Avg',
    max: 'Max',
    collapse: 'Collapse',
    showAll: (count) => `Show all ${count} series`,
  },
  className,
}: MetricsLegendProps) {
  const [expanded, setExpanded] = React.useState(false);

  if (series.length === 0) return null;

  const overLimit = series.length > maxRowsBeforeCollapse;
  const visibleSeries = expanded || !overLimit ? series : series.slice(0, maxRowsBeforeCollapse);

  return (
    <div
      className={cn(
        'mt-2 flex flex-col gap-1 rounded-md border border-bd-0 bg-bg-1 px-3 py-2 font-sans text-[13px]',
        variant === 'inline' &&
          'mt-1 flex-row flex-wrap items-center gap-x-4 gap-y-1 border-0 bg-transparent px-0 py-0',
        variant === 'table' && 'gap-0 overflow-hidden px-0 py-0 text-xs',
        className,
      )}
      role="list"
      aria-label="Series legend"
    >
      {variant === 'table' && (
        <div
          className="type-micro grid grid-cols-[minmax(12rem,1fr)_minmax(4.5rem,auto)_minmax(4.5rem,auto)_minmax(4.5rem,auto)] items-center gap-3 border-b border-bd-0 bg-bg-2/70 px-3 py-1.5 font-medium text-tx-3"
          aria-hidden="true"
        >
          <span>{labels.name}</span>
          <span className="text-right">{labels.current}</span>
          <span className="text-right">{labels.average}</span>
          <span className="text-right">{labels.max}</span>
        </div>
      )}
      {visibleSeries.map((s, i) => {
        const color = colors[i % colors.length] ?? 'var(--chart-1)';
        const stats = seriesStats(s);
        const hidden = hiddenIndexes?.has(i) ?? false;
        const displayName = displayNames?.[i]?.trim();
        const label = displayName || formatSeriesLabel(s);
        const title = displayTitles?.[i] ?? formatSeriesLabel(s);
        return (
          <div
            key={`${i}:${label}`}
            role="listitem"
            className={cn(
              'group flex min-w-0 items-center gap-2 rounded px-1 py-0.5',
              variant === 'inline' && 'max-w-full',
              variant === 'table' &&
                'grid grid-cols-[minmax(12rem,1fr)_minmax(4.5rem,auto)_minmax(4.5rem,auto)_minmax(4.5rem,auto)] gap-3 rounded-none border-b border-bd-0 px-3 py-2 last:border-b-0',
              hidden ? 'opacity-40' : 'opacity-100',
              onToggleIndex ? 'cursor-pointer hover:bg-bg-2' : '',
            )}
            onClick={onToggleIndex ? () => onToggleIndex(i) : undefined}
            onDoubleClick={onSoloIndex ? () => onSoloIndex(i) : undefined}
            onMouseEnter={onHoverIndex ? () => onHoverIndex(i) : undefined}
            onMouseLeave={onHoverIndex ? () => onHoverIndex(null) : undefined}
            title={title}
          >
            <span
              className={cn(
                'flex min-w-0 items-center gap-2',
                variant === 'table' && 'py-0.5',
              )}
            >
              <span
                aria-hidden
                data-testid="metrics-legend-color"
                className="inline-block h-2 w-3 shrink-0 rounded-sm"
                style={{ background: color }}
              />
              {displayName ? (
                <span
                  data-testid="metrics-legend-name"
                  className={cn(
                    'min-w-0 tracking-[0.5px] text-tx-1',
                    variant === 'table'
                      ? 'whitespace-normal break-words leading-5 [overflow-wrap:anywhere]'
                      : 'truncate',
                  )}
                >
                  {displayName}
                </span>
              ) : (
                <SeriesLabelChips
                  labels={s.labels}
                  valueColumn={s.valueColumn}
                  metricName={metricName}
                  metricQuery={metricQuery}
                />
              )}
            </span>
            {variant === 'table' && (
              <>
                <span className="text-right tabular-nums text-tx-1">
                  {stats ? formatTimeSeriesValue(stats.last, unit) : '—'}
                </span>
                <span className="text-right tabular-nums text-tx-2">
                  {stats ? formatTimeSeriesValue(stats.avg, unit) : '—'}
                </span>
                <span className="text-right tabular-nums text-tx-2">
                  {stats ? formatTimeSeriesValue(stats.max, unit) : '—'}
                </span>
              </>
            )}
            {variant !== 'table' && showLastValue && stats !== null && (
              <span className="ml-auto shrink-0 tabular-nums text-tx-2">
                {formatTimeSeriesValue(stats.last, unit)}
              </span>
            )}
          </div>
        );
      })}
      {overLimit && (
        <button
          type="button"
          onClick={() => setExpanded((v) => !v)}
          className={cn(
            'mt-1 self-start rounded px-1.5 py-0.5 text-xs text-tx-2',
            variant === 'inline' && 'mt-0',
            'hover:text-tx-0 focus-visible:bg-bg-2 focus-visible:text-tx-0',
          )}
        >
          {expanded ? labels.collapse : labels.showAll(series.length)}
        </button>
      )}
    </div>
  );
}

function formatSeriesLabel(s: MetricSeries): string {
  const entries = Object.entries(s.labels);
  if (entries.length === 0) return s.valueColumn;
  return entries.map(([k, v]) => `${k}="${v}"`).join(' · ');
}

function SeriesLabelChips({
  labels,
  valueColumn,
  metricName,
  metricQuery,
}: {
  labels: Record<string, string>;
  valueColumn: string;
  metricName?: string | undefined;
  metricQuery?: string | undefined;
}) {
  const entries = Object.entries(labels).sort(([left], [right]) =>
    left.localeCompare(right),
  );
  const name = metricName?.trim() || valueColumn;
  if (entries.length === 0) {
    return <span className="truncate font-mono text-tx-1">{name}</span>;
  }
  return (
    <div className="flex min-w-0 flex-wrap items-center gap-x-1 gap-y-0.5">
      <span className="font-mono font-medium text-tx-1">{name}</span>
      <span className="text-tx-3">{'{'}</span>
      {entries.map(([key, value], idx) => {
        const sigType = detectSignalTypeForLabel(key);
        const sep = idx > 0 ? <span className="text-tx-3">,</span> : null;
        if (sigType) {
          return (
            <React.Fragment key={key}>
              {sep}
              <span className="inline-flex items-center gap-1">
                <span className="text-tx-3">{key}=</span>
                <SignalReference
                  type={sigType}
                  value={value}
                  labelName={key}
                  labels={labels}
                  metricQuery={metricQuery}
                >
                  {JSON.stringify(value)}
                </SignalReference>
              </span>
            </React.Fragment>
          );
        }
        return (
          <React.Fragment key={key}>
            {sep}
            <span className="truncate text-tx-2">
              {key}=<span className="text-tx-1">{JSON.stringify(value)}</span>
            </span>
          </React.Fragment>
        );
      })}
      <span className="text-tx-3">{'}'}</span>
    </div>
  );
}
