import * as React from 'react';

import { cn } from '@/shell/lib/cn';

import { calculateTimeSeriesStats } from '../data';
import type {
  TimeSeriesChartOptions,
  TimeSeriesLegendStat,
} from '../types';
import { LegendViewport } from './LegendViewport';
import type {
  LegendRow,
  TimeSeriesLegendSelectionMode,
  TimeSeriesLegendSeries,
} from './model';
import type { SeriesIdentityConfig } from './SeriesIdentifier';
import { TimeSeriesLegendTable } from './TimeSeriesLegendTable';

interface TimeSeriesLegendProps {
  series: ReadonlyArray<TimeSeriesLegendSeries>;
  mode: TimeSeriesChartOptions['legendMode'];
  placement: TimeSeriesChartOptions['legendPlacement'];
  stats: ReadonlyArray<TimeSeriesLegendStat>;
  density: 'default' | 'compact';
  hiddenIds: ReadonlySet<string>;
  focusedSeriesId: string | null;
  seriesIdentity?: SeriesIdentityConfig;
  onSelect: (
    id: string,
    mode: TimeSeriesLegendSelectionMode,
  ) => void;
  onFocusSeries: (id: string | null) => void;
}

/**
 * Grafana-compatible legend.
 *
 * The structure intentionally follows Grafana's VizLegend: a 14×4 series
 * marker, a flexible name column, min-content calculation columns, weak row
 * separators, and a bottom legend capped at 35% of the visualization height.
 */
export function TimeSeriesLegend({
  series,
  mode,
  placement,
  stats,
  density,
  hiddenIds,
  focusedSeriesId,
  seriesIdentity,
  onSelect,
  onFocusSeries,
}: TimeSeriesLegendProps) {
  const rows = React.useMemo(
    () => series.map((item) => ({
      series: item,
      stats: calculateTimeSeriesStats(item.data),
    })),
    [series],
  );

  if (mode === 'list') {
    return (
      <LegendViewport mode="list" placement={placement}>
        <LegendList
          rows={rows}
          placement={placement}
          hiddenIds={hiddenIds}
          focusedSeriesId={focusedSeriesId}
          onSelect={onSelect}
          onFocusSeries={onFocusSeries}
        />
      </LegendViewport>
    );
  }

  return (
    <LegendViewport
      mode="table"
      placement={placement}
      seriesIdentity={Boolean(seriesIdentity)}
    >
      <TimeSeriesLegendTable
        series={series}
        stats={stats}
        density={density}
        hiddenIds={hiddenIds}
        focusedSeriesId={focusedSeriesId}
        {...(seriesIdentity ? { seriesIdentity } : {})}
        onSelect={onSelect}
        onFocusSeries={onFocusSeries}
      />
    </LegendViewport>
  );
}

function LegendList({
  rows,
  placement,
  hiddenIds,
  focusedSeriesId,
  onSelect,
  onFocusSeries,
}: {
  rows: ReadonlyArray<LegendRow>;
  placement: TimeSeriesChartOptions['legendPlacement'];
  hiddenIds: ReadonlySet<string>;
  focusedSeriesId: string | null;
  onSelect: TimeSeriesLegendProps['onSelect'];
  onFocusSeries: TimeSeriesLegendProps['onFocusSeries'];
}) {
  if (placement === 'right') {
    return (
      <div
        className="flex w-full flex-col gap-y-1 font-sans text-xs"
        role="list"
        aria-label="Series legend"
      >
        {rows.map((row) => (
          <LegendListItem
            key={row.series.id}
            row={row}
            hiddenIds={hiddenIds}
            focusedSeriesId={focusedSeriesId}
            onSelect={onSelect}
            onFocusSeries={onFocusSeries}
          />
        ))}
      </div>
    );
  }
  const left = rows.filter(({ series }) => series.axis !== 'right');
  const right = rows.filter(({ series }) => series.axis === 'right');
  return (
    <div
      className="flex w-full flex-wrap justify-between gap-x-[25px] gap-y-1 font-sans text-xs"
      role="list"
      aria-label="Series legend"
    >
      <div className="flex min-w-0 flex-wrap">
        {left.map((row) => (
          <LegendListItem
            key={row.series.id}
            row={row}
            hiddenIds={hiddenIds}
            focusedSeriesId={focusedSeriesId}
            onSelect={onSelect}
            onFocusSeries={onFocusSeries}
          />
        ))}
      </div>
      {right.length > 0 && (
        <div className="flex min-w-0 flex-1 basis-1/2 flex-wrap justify-end">
          {right.map((row) => (
            <LegendListItem
              key={row.series.id}
              row={row}
              hiddenIds={hiddenIds}
              focusedSeriesId={focusedSeriesId}
              onSelect={onSelect}
              onFocusSeries={onFocusSeries}
            />
          ))}
        </div>
      )}
    </div>
  );
}

function LegendListItem({
  row,
  hiddenIds,
  focusedSeriesId,
  onSelect,
  onFocusSeries,
}: {
  row: LegendRow;
  hiddenIds: ReadonlySet<string>;
  focusedSeriesId: string | null;
  onSelect: TimeSeriesLegendProps['onSelect'];
  onFocusSeries: TimeSeriesLegendProps['onFocusSeries'];
}) {
  const { series: item } = row;
  const hidden = hiddenIds.has(item.id);
  const dimmed = Boolean(focusedSeriesId && focusedSeriesId !== item.id);
  return (
    <span
      role="listitem"
      className={cn(
        'flex min-w-0 items-center gap-2 whitespace-nowrap pr-2.5',
        dimmed && 'opacity-35',
      )}
      onMouseEnter={() => onFocusSeries(item.id)}
      onMouseLeave={() => onFocusSeries(null)}
    >
      <SeriesIcon color={item.color} />
      <LegendLabel
        item={item}
        hidden={hidden}
        onSelect={onSelect}
        onFocusSeries={onFocusSeries}
      />
    </span>
  );
}

function LegendLabel({
  item,
  hidden,
  wrap = false,
  onSelect,
  onFocusSeries,
}: {
  item: TimeSeriesLegendSeries;
  hidden: boolean;
  wrap?: boolean;
  onSelect: TimeSeriesLegendProps['onSelect'];
  onFocusSeries: TimeSeriesLegendProps['onFocusSeries'];
}) {
  return (
    <button
      type="button"
      aria-pressed={!hidden}
      title={item.name}
      onClick={(event) =>
        onSelect(
          item.id,
          event.ctrlKey || event.metaKey || event.shiftKey
            ? 'append'
            : 'isolate',
        )
      }
      onFocus={() => onFocusSeries(item.id)}
      onBlur={() => onFocusSeries(null)}
      className={cn(
        'w-full min-w-0 bg-transparent p-0 text-left font-sans',
        wrap
          ? 'whitespace-normal break-words [overflow-wrap:anywhere]'
          : 'overflow-hidden text-ellipsis whitespace-nowrap',
        'focus-visible:bg-bg-2 focus-visible:text-tx-0',
        hidden ? 'text-tx-4' : 'text-tx-1',
      )}
    >
      {item.name}
    </button>
  );
}

function SeriesIcon({ color }: { color: string }) {
  return (
    <span
      aria-hidden
      className="inline-block h-1 w-3.5 shrink-0 rounded-full align-middle"
      style={{ background: color }}
    />
  );
}
