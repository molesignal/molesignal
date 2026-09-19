import * as React from 'react';

import { cn } from '@/shell/lib/cn';

import { calculateTimeSeriesStats } from '../data';
import { formatTimeSeriesValue } from '../formatters';
import type {
  TimeSeriesLegendStat,
  TimeSeriesStats,
} from '../types';
import type {
  LegendRow,
  TimeSeriesLegendSelectionMode,
  TimeSeriesLegendSeries,
} from './model';
import {
  SeriesIdentifier,
  type SeriesIdentityConfig,
} from './SeriesIdentifier';

type SortColumn = 'name' | TimeSeriesLegendStat;

interface SortState {
  column: SortColumn;
  descending: boolean;
}

interface TimeSeriesLegendTableProps {
  series: ReadonlyArray<TimeSeriesLegendSeries>;
  stats: ReadonlyArray<TimeSeriesLegendStat>;
  density: 'default' | 'compact';
  hiddenIds: ReadonlySet<string>;
  focusedSeriesId: string | null;
  seriesIdentity?: SeriesIdentityConfig;
  onSelect: (id: string, mode: TimeSeriesLegendSelectionMode) => void;
  onFocusSeries: (id: string | null) => void;
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

export function TimeSeriesLegendTable({
  series,
  stats,
  density,
  hiddenIds,
  focusedSeriesId,
  seriesIdentity,
  onSelect,
  onFocusSeries,
}: TimeSeriesLegendTableProps) {
  const [sort, setSort] = React.useState<SortState | null>(null);
  const [expandedIds, setExpandedIds] = React.useState<Set<string>>(
    () => new Set(),
  );
  const resolvedSort =
    sort && sort.column !== 'name' && !stats.includes(sort.column)
      ? null
      : sort;
  const rows = React.useMemo(
    () => sortLegendRows(
      series.map((item) => ({
        series: item,
        stats: calculateTimeSeriesStats(item.data),
      })),
      resolvedSort,
    ),
    [resolvedSort, series],
  );

  return (
    <>
      {seriesIdentity ? (
        <div
          className="sticky top-0 z-20 flex min-h-9 items-center gap-2 border-b border-bd-0 bg-bg-1 px-3 py-1.5 font-sans"
          data-testid="time-series-legend-heading"
        >
          <span className="text-sm font-semibold text-tx-0">
            {seriesIdentity.title}
          </span>
          <span className="text-xs text-tx-3">
            {seriesIdentity.countLabel}
          </span>
        </div>
      ) : null}
      <table
        aria-label="Series legend"
        className={cn(
          'grid w-full whitespace-nowrap text-right font-sans text-xs text-tx-2',
          density === 'compact' && 'type-micro',
        )}
        data-testid="time-series-legend-table"
        style={{
          gridTemplateColumns: legendColumns(
            stats,
            Boolean(seriesIdentity),
          ),
        }}
      >
        <thead
          className={cn(
            'sticky z-10 grid bg-bg-1 text-tx-1',
            seriesIdentity ? 'top-9' : 'top-0',
          )}
          style={subgridStyle}
        >
          <tr className="grid border-b border-bd-0" style={subgridStyle}>
            <th className={legendCellClass(density, 'icon')}>
              <span className="sr-only">Series color</span>
            </th>
            <SortableHeader
              label={seriesIdentity?.nameLabel ?? 'Name'}
              column="name"
              sort={resolvedSort}
              density={density}
              onSort={setSort}
            />
            {stats.map((stat) => (
              <SortableHeader
                key={stat}
                label={seriesIdentity?.statLabels?.[stat] ?? STAT_TITLES[stat]}
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
                <td
                  className={legendCellClass(
                    density,
                    'icon',
                    Boolean(seriesIdentity),
                  )}
                >
                  <SeriesIcon color={item.color} />
                </td>
                <td className={legendCellClass(density, 'name')}>
                  {seriesIdentity ? (
                    <SeriesIdentifier
                      series={item}
                      hidden={hidden}
                      expanded={expandedIds.has(item.id)}
                      text={seriesIdentity}
                      onSelect={(event) =>
                        onSelect(
                          item.id,
                          event.ctrlKey || event.metaKey || event.shiftKey
                            ? 'append'
                            : 'isolate',
                        )}
                      onExpandedChange={(expanded) =>
                        setExpandedIds((current) => {
                          const next = new Set(current);
                          if (expanded) next.add(item.id);
                          else next.delete(item.id);
                          return next;
                        })}
                      onFocusChange={(focused) =>
                        onFocusSeries(focused ? item.id : null)}
                    />
                  ) : (
                    <LegendLabel
                      item={item}
                      hidden={hidden}
                      onSelect={onSelect}
                      onFocusSeries={onFocusSeries}
                    />
                  )}
                </td>
                {stats.map((stat) => (
                  <td
                    key={stat}
                    className={cn(
                      legendCellClass(
                        density,
                        'stat',
                        Boolean(seriesIdentity),
                      ),
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
    </>
  );
}

function LegendLabel({
  item,
  hidden,
  onSelect,
  onFocusSeries,
}: {
  item: TimeSeriesLegendSeries;
  hidden: boolean;
  onSelect: TimeSeriesLegendTableProps['onSelect'];
  onFocusSeries: TimeSeriesLegendTableProps['onFocusSeries'];
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
        )}
      onFocus={() => onFocusSeries(item.id)}
      onBlur={() => onFocusSeries(null)}
      className={cn(
        'w-full min-w-0 whitespace-normal break-words bg-transparent p-0 text-left font-sans [overflow-wrap:anywhere]',
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
  density: TimeSeriesLegendTableProps['density'];
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
          )}
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
  seriesIdentity: boolean,
): string {
  const nameColumn = seriesIdentity
    ? 'minmax(14rem, 1fr)'
    : 'minmax(55px, auto)';
  return `min-content ${nameColumn} ${'min-content '.repeat(stats.length)}`.trim();
}

const subgridStyle: React.CSSProperties = {
  gridColumn: '1 / -1',
  gridTemplateColumns: 'subgrid',
};

function legendCellClass(
  density: TimeSeriesLegendTableProps['density'],
  kind: 'icon' | 'name' | 'stat',
  alignTop = false,
): string {
  return cn(
    'px-2',
    density === 'compact' ? 'py-0' : 'py-0.5',
    kind === 'icon' && cn(
      'flex self-stretch pl-2',
      alignTop ? 'items-start pt-3' : 'items-center',
    ),
    kind === 'name'
      && 'block min-w-0 content-center whitespace-normal text-left',
    kind === 'stat' && cn(
      'block whitespace-nowrap text-right',
      alignTop ? 'content-start pt-3' : 'content-center',
    ),
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
