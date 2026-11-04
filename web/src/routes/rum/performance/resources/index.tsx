import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { widenTimeWindow } from '@/investigation/timeRangeRecovery';
import { ProductState } from '@/product/states';
import { ChromeButton } from '@/shell/chrome';
import { utcOffsetMicros } from '@/shell/query/histogram/axis';

import { useResourcesState } from './address';
import {
  defaultDirection,
  groupOf,
  narrowFilters,
  type Narrowing,
  readRows,
  readStatement,
  type SortKey,
  sortRows,
  statementQuery,
  tableQuery,
} from './model';
import { ResourcePanel } from './Panel';
import { ResourcesStatement } from './Statement';
import { ResourcesTable } from './Table';
import { barsFor } from '../../explorer/viz/spec';
import { CardState } from '../../overview/OverviewCard';
import type { PerformanceContext } from '../Frame';
import { CLOSE_VIEW, openViewChange } from '../openView';
import { useKeptAggregate } from '../useReads';
import { ViewDrawerHost } from '../viewDrawer';

/**
 * What the pages of an application load: how many requests they made, how long they took, how many
 * failed, and the groups of them (a host and a kind, a path, a protocol) with the same numbers each.
 * A group opens what its requests are made of; a request opens the load of the view that made it.
 */
export function ResourcesView({ context }: { context: PerformanceContext }) {
  const { t } = useTranslation('rum');
  const { scope, range, language, clock, window, setWindow } = context;
  const { view, set } = useResourcesState();

  const group = groupOf(view.group);
  const narrowing = React.useMemo<Narrowing>(() => ({ type: view.type, status: view.status, search: view.search.trim() }), [view.search, view.status, view.type]);
  const bars = React.useMemo(
    () => barsFor({ start: range.from, end: range.to }, null, utcOffsetMicros(range.from, clock.tz)),
    [clock.tz, range],
  );

  const statementRead = useKeptAggregate('resources-statement', 'statement', statementQuery(scope, range));
  // The numbers of another grouping or of other words are not these, and never stand in their place.
  const tableRead = useKeptAggregate('resources-table', `${group.key}|${JSON.stringify(narrowFilters(narrowing))}`, tableQuery(scope, range, group, narrowing));

  const statement = readStatement(statementRead.data);
  const direction = view.direction ?? defaultDirection(view.sort);
  const rows = React.useMemo(() => sortRows(readRows(tableRead.data), view.sort, direction), [direction, tableRead.data, view.sort]);
  const open = rows.find((row) => row.key === view.open);

  const onSort = (key: SortKey) => set({ sort: key, dir: key === view.sort ? (direction === 'desc' ? 'asc' : 'desc') : null });

  const empty = statementRead.isSuccess && (statement === null || statement.requests === 0);

  return (
    <>
      {empty ? (
        <ProductState
          variant="empty"
          title={t('performance.resources.empty.title')}
          description={t('overview.empty.description')}
          action={<ChromeButton onClick={() => setWindow(widenTimeWindow(window))}>{t('overview.empty.widen')}</ChromeButton>}
        />
      ) : (
        <>
          <CardState query={statementRead} empty={statement === null} emptyTitle={t('performance.resources.empty.title')}>
            {statement && <ResourcesStatement statement={statement} language={language} />}
          </CardState>
          <div className={open ? 'grid min-w-0 items-start gap-[12px] xl:grid-cols-[minmax(0,1fr)_340px]' : 'min-w-0'}>
            <ResourcesTable
              query={tableRead}
              rows={rows}
              group={group}
              onGroup={(key) => set({ by: key })}
              type={view.type}
              onType={(type) => set({ type: type || null })}
              status={view.status}
              onStatus={(status) => set({ status: status || null })}
              search={view.search}
              onSearch={(search) => set({ q: search.trim() ? search : null })}
              sort={view.sort}
              direction={direction}
              onSort={onSort}
              open={open?.key ?? ''}
              onOpen={(key) => set({ open: key === view.open ? null : key })}
              language={language}
            />
            {open && (
              <ResourcePanel
                key={open.key}
                row={open}
                group={group}
                scope={scope}
                range={range}
                bars={bars}
                narrowing={narrowing}
                language={language}
                onClose={() => set({ open: null })}
                onOpenView={(id, at, request) => set(openViewChange(id, at, request), { push: true })}
              />
            )}
          </div>
        </>
      )}
      <ViewDrawerHost open={view.drawer} onClose={() => set(CLOSE_VIEW)} />
    </>
  );
}
