import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { widenTimeWindow } from '@/investigation/timeRangeRecovery';
import { ProductState } from '@/product/states';
import { ChromeButton } from '@/shell/chrome';
import { utcOffsetMicros } from '@/shell/query/histogram/axis';

import { useFrustrationState } from './address';
import {
  defaultDirection,
  groupOf,
  groupsFor,
  type Kind,
  kindsFor,
  readRows,
  readShare,
  readStatement,
  shareQuery,
  type SortKey,
  sortRows,
  statementQuery,
  tableQuery,
} from './model';
import { FrustrationPanel } from './Panel';
import { FrustrationStatement } from './Statement';
import { FrustrationTable } from './Table';
import { RumInvestigateButton } from '../agent/InvestigateButton';
import { barsFor } from '../explorer/viz/spec';
import { isBrowser } from '../overview/model';
import { CardState } from '../overview/OverviewCard';
import { useKeptAggregate } from '../performance/useReads';
import { type ScopeContext, ScopedFrame } from '../ScopedFrame';

/**
 * The tab of what frustrates the people who use an application: what share of the sessions showed a
 * sign, how many actions showed each sign and in how many sessions, and the groups of them (the elements,
 * the pages, the versions, the browsers) by the sessions each touched. A group opens what its actions
 * are made of and the newest sessions that met them. An SDK judges the signs where the person is; the
 * server does not judge them again.
 */
export function RumFrustration() {
  return (
    <ScopedFrame
      name="frustration"
      toolbar={(context) => (
        <RumInvestigateButton
          subject={{
            application: { id: context.application.id, name: context.application.name },
            environment: context.scope.environment,
            range: context.range,
            focus: { kind: 'frustration' },
          }}
        />
      )}
    >
      {(context) => <FrustrationView context={context} />}
    </ScopedFrame>
  );
}

function FrustrationView({ context }: { context: ScopeContext }) {
  const { t } = useTranslation('rum');
  const { application, scope, range, language, clock, window, setWindow } = context;
  const { view, set } = useFrustrationState();

  const browser = isBrowser(application.platform);
  const group = groupOf(application.platform, view.group);
  const search = view.search.trim();
  const narrowing = React.useMemo(() => ({ kind: view.kind, search }), [search, view.kind]);
  const bars = React.useMemo(
    () => barsFor({ start: range.from, end: range.to }, null, utcOffsetMicros(range.from, clock.tz)),
    [clock.tz, range],
  );

  const statementRead = useKeptAggregate('frustration-statement', 'statement', statementQuery(scope, range));
  const shareRead = useKeptAggregate('frustration-share', 'share', shareQuery(scope, range));
  // The numbers of another grouping, another sign or other words are not these, and never stand in their place.
  const tableRead = useKeptAggregate('frustration-table', `${group.key}|${view.kind}|${search}`, tableQuery(scope, range, group, narrowing));

  const statement = readStatement(statementRead.data);
  const share = readShare(shareRead.data);
  const direction = view.direction ?? defaultDirection();
  const rows = React.useMemo(() => sortRows(readRows(tableRead.data), view.sort, direction), [direction, tableRead.data, view.sort]);
  const open = rows.find((row) => row.key === view.open);

  const onSort = (key: SortKey) => set({ sort: key, dir: key === view.sort ? (direction === 'desc' ? 'asc' : 'desc') : null });

  const empty = statementRead.isSuccess && (statement === null || statement.kinds.length === 0);

  if (empty) {
    return (
      <ProductState
        variant="empty"
        title={t('frustration.empty.title')}
        description={t(browser ? 'frustration.empty.description_click' : 'frustration.empty.description_tap')}
        action={<ChromeButton onClick={() => setWindow(widenTimeWindow(window))}>{t('overview.empty.widen')}</ChromeButton>}
      />
    );
  }

  return (
    <>
      <CardState query={statementRead} empty={statement === null} emptyTitle={t('frustration.empty.title')}>
        {statement && <FrustrationStatement statement={statement} share={share !== null && share > 0 ? share : null} browser={browser} language={language} />}
      </CardState>
      <div className={open ? 'grid min-w-0 items-start gap-[12px] xl:grid-cols-[minmax(0,1fr)_340px]' : 'min-w-0'}>
        <FrustrationTable
          query={tableRead}
          rows={rows}
          group={group}
          groups={groupsFor(application.platform)}
          kinds={kindsFor(application.platform)}
          onGroup={(key) => set({ by: key })}
          kind={view.kind}
          onKind={(kind: Kind | '') => set({ kind: kind || null })}
          search={view.search}
          onSearch={(words) => set({ q: words.trim() ? words : null })}
          sort={view.sort}
          direction={direction}
          onSort={onSort}
          open={open?.key ?? ''}
          onOpen={(key) => set({ open: key === view.open ? null : key })}
          browser={browser}
          language={language}
        />
        {open && (
          <FrustrationPanel
            key={open.key}
            row={open}
            group={group}
            platform={application.platform}
            scope={scope}
            range={range}
            bars={bars}
            narrowing={narrowing}
            language={language}
            onClose={() => set({ open: null })}
          />
        )}
      </div>
    </>
  );
}
