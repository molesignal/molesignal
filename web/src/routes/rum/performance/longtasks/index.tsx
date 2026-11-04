import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { widenTimeWindow } from '@/investigation/timeRangeRecovery';
import { ProductState } from '@/product/states';
import { ChromeButton } from '@/shell/chrome';

import { useLongTasksState } from './address';
import { defaultDirection, groupOf, readRows, readShare, readStatement, type SortKey, sortRows, statementQuery, tableQuery, viewsQuery } from './model';
import { LongTaskPanel } from './Panel';
import { LongTasksStatement } from './Statement';
import { LongTasksTable } from './Table';
import { CardState } from '../../overview/OverviewCard';
import type { PerformanceContext } from '../Frame';
import { CLOSE_VIEW, openViewChange } from '../openView';
import { useKeptAggregate } from '../useReads';
import { ViewDrawerHost } from '../viewDrawer';

/**
 * What blocks the pages of an application: how many long tasks the main thread ran, how many of the page
 * views met one, and the groups of them (the script and function a task is put down to, a page, a
 * version, how it was triggered) with how long each ran. A group opens what its long tasks are made of;
 * a long task opens the load of the view it blocked.
 */
export function LongTasksView({ context }: { context: PerformanceContext }) {
  const { t } = useTranslation('rum');
  const { scope, range, language, window, setWindow } = context;
  const { view, set } = useLongTasksState();

  const group = groupOf(view.group);
  const search = view.search.trim();

  const statementRead = useKeptAggregate('long-tasks-statement', 'statement', statementQuery(scope, range));
  const shareRead = useKeptAggregate('long-tasks-share', 'share', viewsQuery(scope, range));
  // The numbers of another grouping or of other words are not these, and never stand in their place.
  const tableRead = useKeptAggregate('long-tasks-table', `${group.key}|${search}`, tableQuery(scope, range, group, search));

  const statement = readStatement(statementRead.data);
  const share = readShare(shareRead.data);
  const direction = view.direction ?? defaultDirection();
  const rows = React.useMemo(() => sortRows(readRows(tableRead.data), view.sort, direction), [direction, tableRead.data, view.sort]);
  const open = rows.find((row) => row.key === view.open);

  const onSort = (key: SortKey) => set({ sort: key, dir: key === view.sort ? (direction === 'desc' ? 'asc' : 'desc') : null });

  const empty = statementRead.isSuccess && (statement === null || statement.tasks === 0);

  return (
    <>
      {empty ? (
        <ProductState
          variant="empty"
          title={t('performance.long_tasks.empty.title')}
          description={t('performance.long_tasks.empty.description')}
          action={<ChromeButton onClick={() => setWindow(widenTimeWindow(window))}>{t('overview.empty.widen')}</ChromeButton>}
        />
      ) : (
        <>
          <CardState query={statementRead} empty={statement === null} emptyTitle={t('performance.long_tasks.empty.title')}>
            {statement && <LongTasksStatement statement={statement} share={share} language={language} />}
          </CardState>
          <div className={open ? 'grid min-w-0 items-start gap-[12px] xl:grid-cols-[minmax(0,1fr)_340px]' : 'min-w-0'}>
            <LongTasksTable
              query={tableRead}
              rows={rows}
              group={group}
              onGroup={(key) => set({ by: key })}
              search={view.search}
              onSearch={(words) => set({ q: words.trim() ? words : null })}
              sort={view.sort}
              direction={direction}
              onSort={onSort}
              open={open?.key ?? ''}
              onOpen={(key) => set({ open: key === view.open ? null : key })}
              language={language}
            />
            {open && (
              <LongTaskPanel
                key={open.key}
                row={open}
                group={group}
                scope={scope}
                range={range}
                search={search}
                language={language}
                onClose={() => set({ open: null })}
                onOpenView={(id, at, task) => set(openViewChange(id, at, task), { push: true })}
              />
            )}
          </div>
        </>
      )}
      <ViewDrawerHost open={view.drawer} onClose={() => set(CLOSE_VIEW)} />
    </>
  );
}
