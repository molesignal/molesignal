import { ChevronRight } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import { formatDuration, formatRate } from '@/routes/apm/format';
import { TableShell, Td, Th, Tr } from '@/shell/chrome';
import { cn } from '@/shell/lib/cn';
import { StatusWord, type StatusTone } from '@/shell/StatusWord';
import { ViewTabs } from '@/shell/tabs/ViewTabs';
import { FittedSelect } from '@/shell/ui/fitted-select';
import { useUsers } from '@/shell/useUsers';
import { formatRelativeMicros } from '@/time/relative';

import {
  RESOURCE_SCOPES,
  RESOURCE_TABS,
  type ResourceRow,
  type ResourceScope,
  type ResourceState,
  type ResourceTab,
  scopedRows,
} from './model';
import { useResourceRows } from './useResourceRows';
import { useRowsThatFit } from './useRowsThatFit';

const STATE_TONE: Record<ResourceState, StatusTone> = {
  healthy: 'green',
  warning: 'yellow',
  degraded: 'yellow',
  flaky: 'yellow',
  critical: 'red',
  failing: 'red',
  no_traffic: 'dim',
  unknown: 'dim',
};

type Column = 'state' | 'detail' | 'requestRate' | 'errorRate' | 'p95Time' | 'updatedAt' | 'owner';

/** What each tab says of a row besides its name. */
const COLUMNS: Record<ResourceTab, readonly Column[]> = {
  services: ['state', 'requestRate', 'errorRate', 'p95Time', 'owner'],
  dashboards: ['updatedAt', 'owner'],
  saved_views: ['detail', 'updatedAt', 'owner'],
  checks: ['state', 'detail', 'updatedAt', 'owner'],
  status_pages: ['detail', 'updatedAt'],
};

const NUMERIC: ReadonlySet<Column> = new Set(['requestRate', 'errorRate', 'p95Time']);

function formatPerSecond(value: number): string {
  return new Intl.NumberFormat(undefined, {
    maximumFractionDigits: value >= 100 ? 0 : value >= 10 ? 1 : 2,
  }).format(value);
}

/**
 * The reader's resources, a tab a kind: services, dashboards, saved queries, checks, status pages.
 * The scope says whose; when it holds none, the next scope that does is shown and named.
 */
export function ResourcesCard({ className }: { className?: string }) {
  const { t, i18n } = useTranslation('onboarding');
  const locale = i18n.resolvedLanguage ?? i18n.language;
  const { nameOf } = useUsers();
  const { rows: rowsByTab, reader, loading } = useResourceRows();
  const tabs = RESOURCE_TABS.filter((tab) => rowsByTab.has(tab));
  const [chosenTab, setTab] = React.useState<ResourceTab | null>(null);
  // Until a tab is chosen, the first that has something opens; one still loading is waited for,
  // so the card does not jump from a later tab back to it.
  const firstFilled = tabs.find((key) => loading[key] || (rowsByTab.get(key)?.length ?? 0) > 0);
  const tab = chosenTab && tabs.includes(chosenTab) ? chosenTab : (firstFilled ?? tabs[0] ?? 'services');
  const [scope, setScope] = React.useState<ResourceScope>('mine');
  const shown = scopedRows(rowsByTab.get(tab) ?? [], scope, reader);
  const bodyRef = React.useRef<HTMLDivElement>(null);
  const fitting = useRowsThatFit(bodyRef);
  const visible = shown.rows.slice(0, fitting);
  const hidden = shown.rows.length - visible.length;
  const columns = COLUMNS[tab];
  const nowMicros = Date.now() * 1000;

  const cell = (row: ResourceRow, column: Column): React.ReactNode => {
    switch (column) {
      case 'state':
        return row.state ? (
          <StatusWord tone={STATE_TONE[row.state]} strong={STATE_TONE[row.state] === 'red'}>
            {t(`workbench.resources.states.${row.state}`)}
          </StatusWord>
        ) : (
          '—'
        );
      case 'detail':
        return row.detail ?? '—';
      case 'requestRate':
        return row.requestRate === undefined ? '—' : formatPerSecond(row.requestRate);
      case 'errorRate':
        return row.errorRate === undefined ? '—' : formatRate(row.errorRate);
      case 'p95Time':
        return formatDuration(row.p95Time);
      case 'updatedAt':
        return formatRelativeMicros(row.updatedAt, locale, nowMicros);
      case 'owner':
        if (!row.ownerId) return '—';
        return row.ownerId === reader.userId ? t('workbench.resources.me') : nameOf(row.ownerId) || '—';
    }
  };

  return (
    <section
      aria-label={t('workbench.resources.title')}
      data-testid="workbench-resources"
      data-home-surface="section"
      className={cn(
        'flex min-h-0 min-w-0 flex-col overflow-hidden rounded-md bg-[var(--functional-surface)] [box-shadow:var(--shadow-functional-surface)]',
        className,
      )}
    >
      {/* One row on a wide card; on a narrow one the tabs take a row of their own under the title. */}
      <div className="flex min-h-[48px] shrink-0 flex-wrap items-center gap-x-4 gap-y-1 px-[20px] py-2">
        <h2 className="type-section-title shrink-0 font-sans font-display text-tx-0">
          {t('workbench.resources.title')}
        </h2>
        {tabs.length > 0 && (
          <>
            <div className="order-last min-w-0 basis-full md:order-none md:flex-1 md:basis-0">
              <ViewTabs
                label={t('workbench.resources.kinds_label')}
                items={tabs.map((key) => ({
                  key,
                  label: t(`workbench.resources.tabs.${key}`),
                  count: loading[key] ? undefined : scopedRows(rowsByTab.get(key) ?? [], scope, reader).rows.length,
                }))}
                value={tab}
                onValueChange={setTab}
              />
            </div>
            <FittedSelect
              value={scope}
              onValueChange={(value) => setScope(value as ResourceScope)}
              options={RESOURCE_SCOPES.map((value) => ({ value, label: t(`workbench.resources.scopes.${value}`) }))}
              maxWidth={160}
              className="ml-auto"
              aria-label={t('workbench.resources.scope_label')}
              data-testid="workbench-resources-scope"
            />
          </>
        )}
      </div>
      {shown.scope !== scope && shown.rows.length > 0 && (
        <p className="shrink-0 px-[20px] pb-2 font-sans text-xs text-tx-3" data-testid="workbench-resources-fallback">
          {t('workbench.resources.fallback', {
            chosen: t(`workbench.resources.scopes.${scope}`),
            shown: t(`workbench.resources.scopes.${shown.scope}`),
          })}
        </p>
      )}
      <div ref={bodyRef} className="min-h-0 flex-1 overflow-hidden">
        {tabs.length === 0 ? (
          <p className="px-[20px] py-6 font-sans text-sm text-tx-3" data-testid="workbench-resources-no-access">
            {t('workbench.resources.no_access')}
          </p>
        ) : shown.rows.length === 0 ? (
          <p className="px-[20px] py-6 font-sans text-sm text-tx-3">
            {loading[tab] ? t('workbench.loading') : t(`workbench.resources.empty.${tab}`)}
          </p>
        ) : (
          <TableShell className="min-w-[560px]" containerClassName="overflow-y-hidden">
            <thead>
              <tr>
                <Th className="pl-[20px]">{t(`workbench.resources.tabs.${tab}`)}</Th>
                {columns.map((column) => (
                  <Th key={column} className={cn(NUMERIC.has(column) && 'text-right', column === 'owner' && 'pl-6')}>
                    {t(`workbench.resources.columns.${column}`)}
                  </Th>
                ))}
                <Th className="w-10 pr-[20px]">
                  <span className="sr-only">{t('workbench.open')}</span>
                </Th>
              </tr>
            </thead>
            <tbody>
              {visible.map((row) => (
                <Tr key={row.key} className="group">
                  <Td className="max-w-[280px] pl-[20px]">
                    <Link to={row.path} className="block truncate font-strong text-tx-0 hover:text-[var(--blue-ink)]">
                      {row.name}
                    </Link>
                  </Td>
                  {columns.map((column) => (
                    <Td
                      key={column}
                      className={cn(
                        'whitespace-nowrap',
                        NUMERIC.has(column) ? 'text-right tabular-nums' : '',
                        column === 'owner' && 'pl-6',
                        column === 'owner' && row.ownerId === reader.userId ? 'text-tx-0' : 'text-tx-3',
                      )}
                    >
                      {cell(row, column)}
                    </Td>
                  ))}
                  <Td className="pr-[20px] text-right">
                    <ChevronRight
                      aria-hidden="true"
                      className="ml-auto h-4 w-4 text-tx-3 transition-[color,transform] duration-fast group-hover:translate-x-0.5 group-hover:text-tx-1"
                    />
                  </Td>
                </Tr>
              ))}
            </tbody>
          </TableShell>
        )}
      </div>
      {hidden > 0 && (
        <Link
          to={listPath(tab)}
          className="flex h-8 shrink-0 items-center gap-1 px-[20px] font-sans text-xs font-strong text-tx-2 transition-colors duration-fast hover:text-tx-0"
        >
          {t('workbench.resources.view_all', { count: shown.rows.length, kind: t(`workbench.resources.tabs.${tab}`) })}
          <ChevronRight aria-hidden="true" className="h-3 w-3" />
        </Link>
      )}
    </section>
  );
}

/** Where every resource of a tab is listed. */
function listPath(tab: ResourceTab): string {
  switch (tab) {
    case 'services':
      return '/apm/services';
    case 'dashboards':
      return '/dashboards';
    case 'saved_views':
      return '/saved-views';
    case 'checks':
      return '/synthetics';
    case 'status_pages':
      return '/status-pages';
  }
}
