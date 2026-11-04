import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { widenTimeWindow } from '@/investigation/timeRangeRecovery';
import { ProductState } from '@/product/states';
import { ChromeButton } from '@/shell/chrome';
import { utcOffsetMicros } from '@/shell/query/histogram/axis';

import { usePerformanceView } from './address';
import { AttributionDrawer } from './drawer/AttributionDrawer';
import type { PerformanceContext } from './Frame';
import { GradesCard } from './Grades';
import {
  CORE_METRICS,
  defaultDirection,
  dimensionOf,
  drawerMetricOf,
  levelsQuery,
  type MetricKey,
  metricOf,
  readLevels,
  readRows,
  readViews,
  type SortKey,
  sortRows,
  SUPPORT_METRICS,
  tableQuery,
  trendQuery,
} from './model';
import { CLOSE_VIEW, openViewChange } from './openView';
import { PerformanceStatement } from './Statement';
import { PerformanceTable, valueLabel } from './Table';
import { TrendCard } from './Trend';
import { useKeptAggregate } from './useReads';
import { ViewDrawerHost } from './viewDrawer';
import { barsFor } from '../explorer/viz/spec';
import { CardState } from '../overview/OverviewCard';

/**
 * How the pages of an application do for the people who open them: where the Core Web Vitals stand,
 * how they went over the time in view, how the views divide among good, needs improvement and poor,
 * and which pages (or browsers, countries, versions) are slow. A row opens what its views are made
 * of: the part of the level that takes the time, the element that sets it, and the views that were
 * worst.
 */
export function VitalsView({ context }: { context: PerformanceContext }) {
  const { t } = useTranslation('rum');
  const { scope, range, language, clock, window, setWindow } = context;
  const { view, set } = usePerformanceView();

  const metric = metricOf(view.metric);
  const dimension = dimensionOf(view.by);
  const bars = React.useMemo(
    () => barsFor({ start: range.from, end: range.to }, null, utcOffsetMicros(range.from, clock.tz)),
    [clock.tz, range],
  );

  const core = useKeptAggregate('core', 'core', levelsQuery(scope, range, CORE_METRICS));
  const support = useKeptAggregate('support', 'support', levelsQuery(scope, range, SUPPORT_METRICS));
  const trend = useKeptAggregate('trend', metric.key, trendQuery(scope, bars, metric));
  const table = useKeptAggregate('table', dimension.key, tableQuery(scope, range, dimension));

  const views = readViews(core.data);
  const levels = [...readLevels(core.data, CORE_METRICS), ...readLevels(support.data, SUPPORT_METRICS)];

  const direction = view.direction ?? defaultDirection(view.sort);
  const none = t('performance.table.none');
  const words = view.search.trim().toLowerCase();
  const rows = React.useMemo(() => readRows(table.data), [table.data]);
  const shown = React.useMemo(
    () =>
      sortRows(
        words === '' ? rows : rows.filter((row) => valueLabel(dimension.key, row.value, language, none).toLowerCase().includes(words)),
        view.sort,
        direction,
        view.metric,
      ),
    [dimension.key, direction, language, none, rows, view.metric, view.sort, words],
  );

  const onSort = (key: SortKey) => {
    const level = key !== 'views' && key !== 'good' ? (key as MetricKey) : null;

    // Ordering by a level takes the page to it, so that the chart and the grades are of what is ordered.
    set({ sort: key, dir: key === view.sort ? (direction === 'desc' ? 'asc' : 'desc') : null, ...(level && { vital: level }) });
  };

  const defaultTab = drawerMetricOf(null, view.metric);

  const body = (
    <CardState query={core} empty={false} emptyTitle="">
      {core.data && (
        <>
          <PerformanceStatement views={views} levels={levels} metric={view.metric} onPick={(key) => set({ vital: key })} language={language} />
          <div className="grid min-w-0 gap-[12px] xl:grid-cols-[minmax(0,1.6fr)_minmax(0,1fr)]">
            <TrendCard
              query={trend}
              bars={bars}
              metric={metric}
              language={language}
              onRangeSelect={({ from, to }) =>
                setWindow({ mode: 'absolute', from: new Date(from / 1_000).toISOString(), to: new Date(to / 1_000).toISOString() })
              }
            />
            <GradesCard query={core} levels={levels} metric={view.metric} onPick={(key) => set({ vital: key })} language={language} />
          </div>
          <PerformanceTable
            query={table}
            rows={shown}
            total={rows.length}
            dimension={dimension.key}
            onDimension={(key) => set({ by: key })}
            metric={metric}
            sort={view.sort}
            direction={direction}
            onSort={onSort}
            search={view.search}
            onSearch={(search) => set({ q: search.trim() ? search : null })}
            open={view.open}
            onOpen={(value) => set({ open: value }, { push: true })}
            subject={{ application: { id: context.application.id, name: context.application.name }, environment: scope.environment, range }}
            language={language}
          />
        </>
      )}
    </CardState>
  );

  return (
    <>
      {core.isSuccess && views === 0 ? (
        <ProductState
          variant="empty"
          title={t('performance.empty.title')}
          description={t('overview.empty.description')}
          action={<ChromeButton onClick={() => setWindow(widenTimeWindow(window))}>{t('overview.empty.widen')}</ChromeButton>}
        />
      ) : (
        body
      )}
      <AttributionDrawer
        value={view.open}
        dimension={view.by}
        tab={view.tab}
        onTab={(tab) => set({ tab: tab === defaultTab ? null : tab })}
        scope={scope}
        range={range}
        bars={bars}
        language={language}
        onClose={() => set({ open: null, tab: null })}
        onOpenView={(id, at) => set(openViewChange(id, at, ''), { push: true })}
      />
      <ViewDrawerHost open={view.drawer} onClose={() => set(CLOSE_VIEW)} />
    </>
  );
}
