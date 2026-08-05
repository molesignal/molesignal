import * as React from 'react';

import { cn } from '@/shell/lib/cn';

import { calculateTimeSeriesStats } from '../data';
import { formatTimeSeriesValue } from '../formatters';
import type {
  TimeSeriesChartOptions,
  TimeSeriesLegendStat,
  TimeSeriesSeries,
  TimeSeriesStats,
} from '../types';
import { LegendViewport } from './LegendViewport';

export interface TimeSeriesLegendSeries extends TimeSeriesSeries {
  id: string;
  color: string;
}

export type TimeSeriesLegendSelectionMode = 'isolate' | 'append';

interface TimeSeriesLegendProps {
  series: ReadonlyArray<TimeSeriesLegendSeries>;
  mode: TimeSeriesChartOptions['legendMode'];
  placement: TimeSeriesChartOptions['legendPlacement'];
  stats: ReadonlyArray<TimeSeriesLegendStat>;
  density: 'default' | 'compact';
  hiddenIds: ReadonlySet<string>;
  focusedSeriesId: string | null;
  onSelect: (
    id: string,
    mode: TimeSeriesLegendSelectionMode,
  ) => void;
  onFocusSeries: (id: string | null) => void;
}

interface LegendRow {
  series: TimeSeriesLegendSeries;
  stats: TimeSeriesStats;
}

type SortColumn = 'name' | TimeSeriesLegendStat;

interface SortState {
  column: SortColumn;
  descending: boolean;
}

const naturalCompare = new Intl.Collator(undefined, {
  numeric: true,
  sensitivity: 'base',
}).compare;

const STAT_TITLES: Record<TimeSeriesLegendStat, string> = {
  last: 'Last',
  min: 'Min',
  max: 'Max',
  mean: 'Mean',
  sum: 'Total',
};

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
  onSelect,
  onFocusSeries,
}: TimeSeriesLegendProps) {
  const [sort, setSort] = React.useState<SortState | null>(null);
  const resolvedSort =
    sort && sort.column !== 'name' && !stats.includes(sort.column)
      ? null
      : sort;
  const rows = React.useMemo(
    () =>
      sortLegendRows(
        series.map((item) => ({
          series: item,
          stats: calculateTimeSeriesStats(item.data),
        })),
        resolvedSort,
      ),
    [resolvedSort, series],
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
    <LegendViewport mode="table" placement={placement}>
      <table
        aria-label="Series legend"
        className={cn(
          'grid w-full whitespace-nowrap text-right font-sans text-xs text-tx-2',
          density === 'compact' && 'type-micro',
        )}
        data-testid="time-series-legend-table"
        style={{ gridTemplateColumns: legendColumns(stats) }}
      >
        <thead
          className="sticky top-0 z-10 grid bg-bg-1 text-tx-1"
          style={subgridStyle}
        >
          <tr className="grid border-b border-bd-0" style={subgridStyle}>
            <th className={legendCellClass(density, 'icon')}>
              <span className="sr-only">Series color</span>
            </th>
            <SortableHeader
              label="Name"
              column="name"
              sort={resolvedSort}
              density={density}
              onSort={setSort}
            />
            {stats.map((stat) => (
              <SortableHeader
                key={stat}
                label={STAT_TITLES[stat]}
                column={stat}
                sort={resolvedSort}
                density={density}
                align="right"
                onSort={setSort}
              />
            ))}
          </tr>
        </thead>
        <tbody className="grid" style={subgridStyle}>
          {rows.map(({ series: item, stats: itemStats }) => {
            const hidden = hiddenIds.has(item.id);
            const dimmed = Boolean(
              focusedSeriesId && focusedSeriesId !== item.id,
            );
            return (
              <tr
                key={item.id}
                className={cn(
                  'grid border-b border-bd-0 transition-colors last:border-b-0 hover:bg-bg-2',
                  dimmed && 'opacity-35',
                )}
                style={subgridStyle}
                onMouseEnter={() => onFocusSeries(item.id)}
                onMouseLeave={() => onFocusSeries(null)}
              >
                <td className={legendCellClass(density, 'icon')}>
                  <SeriesIcon color={item.color} />
                </td>
                <td className={legendCellClass(density, 'name')}>
                  <LegendLabel
                    item={item}
                    hidden={hidden}
                    wrap
                    onSelect={onSelect}
                    onFocusSeries={onFocusSeries}
                  />
                </td>
                {stats.map((stat) => (
                  <td
                    key={stat}
                    className={cn(
                      legendCellClass(density, 'stat'),
                      'tabular-nums',
                    )}
                  >
                    {formatLegendStat(itemStats, stat, item.unit)}
                  </td>
                ))}
              </tr>
            );
          })}
        </tbody>
      </table>
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

function SortableHeader({
  label,
  column,
  sort,
  density,
  align = 'left',
  onSort,
}: {
  label: string;
  column: SortColumn;
  sort: SortState | null;
  density: TimeSeriesLegendProps['density'];
  align?: 'left' | 'right';
  onSort: React.Dispatch<React.SetStateAction<SortState | null>>;
}) {
  const active = sort?.column === column;
  return (
    <th
      className={legendCellClass(
        density,
        align === 'right' ? 'stat' : 'name',
      )}
      aria-sort={
        active ? (sort.descending ? 'descending' : 'ascending') : 'none'
      }
    >
      <button
        type="button"
        className={cn(
          'flex w-full items-center gap-0.5 bg-transparent p-0 font-medium text-tx-1',
          align === 'right' ? 'justify-end text-right' : 'text-left',
          'focus-visible:bg-bg-2 focus-visible:text-tx-0',
        )}
        onClick={() =>
          onSort((current) =>
            current?.column === column
              ? { column, descending: !current.descending }
              : { column, descending: false },
          )
        }
      >
        {label}
        {active && <span aria-hidden>{sort.descending ? '▾' : '▴'}</span>}
      </button>
    </th>
  );
}

function sortLegendRows(
  rows: LegendRow[],
  sort: SortState | null,
): LegendRow[] {
  if (!sort) return rows;
  const direction = sort.descending ? -1 : 1;
  return rows.sort((left, right) => {
    if (sort.column === 'name') {
      return direction * naturalCompare(left.series.name, right.series.name);
    }
    const leftValue = left.stats[sort.column];
    const rightValue = right.stats[sort.column];
    if (leftValue === null) return rightValue === null ? 0 : 1;
    if (rightValue === null) return -1;
    return direction * (leftValue - rightValue);
  });
}

function legendColumns(
  stats: ReadonlyArray<TimeSeriesLegendStat>,
): string {
  return `min-content minmax(55px, auto) ${'min-content '.repeat(stats.length)}`.trim();
}

const subgridStyle: React.CSSProperties = {
  gridColumn: '1 / -1',
  gridTemplateColumns: 'subgrid',
};

function legendCellClass(
  density: TimeSeriesLegendProps['density'],
  kind: 'icon' | 'name' | 'stat',
): string {
  return cn(
    'px-2',
    density === 'compact' ? 'py-0' : 'py-0.5',
    kind === 'icon' && 'flex self-stretch items-center pl-2',
    kind === 'name' &&
      'block min-w-0 content-center whitespace-normal text-left',
    kind === 'stat' &&
      'block content-center whitespace-nowrap text-right',
  );
}

function formatLegendStat(
  stats: TimeSeriesStats,
  stat: TimeSeriesLegendStat,
  unit: string | undefined,
): string {
  const value = stats[stat];
  return typeof value === 'number'
    ? formatTimeSeriesValue(value, unit)
    : '—';
}
