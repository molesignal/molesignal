import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { RefreshCw, Trash2 } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useSearchParams } from 'react-router-dom';

import {
  ClearFiltersButton,
  ConfirmDialog,
  DataTable,
  TableBulkBar,
  TableExportButton,
  runOverRows,
  useTableSelection,
  useTableSort,
} from '@/admin';
import * as usageApi from '@/api/intakeUsage';
import type { SourceUsage, UsageDimension } from '@/api/intakeUsage';
import { toApiError } from '@/lib/http';
import { useTimeFormatter } from '@/lib/time';
import { useProductAccess } from '@/product/access';
import { useActionAccess } from '@/product/actionAccess';
import type { ProductStateProps } from '@/product/states';
import { ListPage } from '@/product/templates';
import { ChromeButton, QueryInput } from '@/shell/chrome';
import { ViewTabs } from '@/shell/tabs/ViewTabs';
import { FittedSelect } from '@/shell/ui/fitted-select';
import { toast } from '@/shell/ui/sonner';

import { StreamPagination, useStreamPagination } from '../StreamPagination';
import { BudgetDrawer } from './BudgetDrawer';
import { usageColumns } from './columns';
import {
  USAGE_DIMENSIONS,
  USAGE_WINDOWS,
  bytesFigure,
  countFigure,
  dimensionFromParam,
  matchesSearch,
  windowFromParam,
} from './model';
import { SourceDrawer } from './SourceDrawer';

const USAGE_KEY = ['intake-usage'] as const;

/**
 * What each source took in over a window, and how it stands against its budget.
 * Three views of the same figures: by credential, by stream, by service. A budget
 * only tells people; intake goes on the same whatever it says.
 */
export function IntakeUsage() {
  const { t } = useTranslation('streams');
  const queryClient = useQueryClient();
  const formatter = useTimeFormatter();
  const access = useProductAccess();
  const manageAccess = useActionAccess({ permission: 'intake_budgets.manage' });
  const [searchParams, setSearchParams] = useSearchParams();
  const dimension = dimensionFromParam(searchParams.get('dimension'));
  const usageWindow = windowFromParam(searchParams.get('window'));
  const [search, setSearch] = React.useState('');
  const [opened, setOpened] = React.useState<SourceUsage | null>(null);
  const [budgeting, setBudgeting] = React.useState<readonly SourceUsage[]>([]);
  const [removing, setRemoving] = React.useState<readonly SourceUsage[]>([]);

  // The platform organization has no notify policies: nobody would hear of a budget there.
  const withBudgets = access?.scope !== 'system';
  const canManage = withBudgets && manageAccess.allowed;

  const query = useQuery({
    queryKey: [...USAGE_KEY, dimension, usageWindow.key],
    queryFn: () => usageApi.overview({ dimension, windowSecs: usageWindow.secs }),
    refetchInterval: 60_000,
  });

  const refresh = () => queryClient.invalidateQueries({ queryKey: USAGE_KEY });

  const setParam = (name: string, value: string | null) => {
    setSearchParams(
      (current) => {
        const next = new URLSearchParams(current);
        if (value === null) next.delete(name);
        else next.set(name, value);
        return next;
      },
      { replace: true },
    );
  };

  const changeDimension = (next: UsageDimension) => {
    setSearch('');
    setOpened(null);
    setParam('dimension', next === 'credential' ? null : next);
  };

  const sources = React.useMemo(() => query.data?.sources ?? [], [query.data]);

  const filtered = React.useMemo(
    () => sources.filter((row) => matchesSearch(t, row, search)),
    [search, sources, t],
  );

  const columns = usageColumns({
    t,
    withBudgets,
    canManage,
    onSetBudget: (row) => setBudgeting([row]),
    onRemoveBudget: (row) => setRemoving([row]),
  });

  const { sort, setSort, sortedRows } = useTableSort(filtered, columns);
  const pagination = useStreamPagination(sortedRows, `${dimension}:${usageWindow.key}:${search}`);

  const { selection, selectedRows, clear: clearSelection } = useTableSelection(
    sortedRows,
    (row) => row.sourceKey,
    (row) => row.budgetable || row.budget !== null,
  );

  // The row in the drawer follows the list as it reads itself again.
  const openedRow = opened ? (sources.find((row) => row.sourceKey === opened.sourceKey) ?? opened) : null;

  const remove = useMutation({
    mutationFn: (rows: readonly SourceUsage[]) =>
      runOverRows(
        rows.flatMap((row) => (row.budget ? [row.budget.id] : [])),
        (id) => usageApi.removeBudget(id),
      ),
    onSuccess: async (outcome) => {
      if (outcome.failed === 0) {
        toast.success(t('usage.budget.removed', { count: outcome.done }));
        clearSelection();
      } else {
        toast.error(
          t('usage.budget.remove_failed', {
            count: outcome.failed,
            total: outcome.done + outcome.failed,
            reason: toApiError(outcome.firstError).message,
          }),
        );
      }

      setRemoving([]);
      await refresh();
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const data = query.data;

  const state: ProductStateProps | null = query.isLoading
    ? { variant: 'loading' }
    : query.isError
      ? {
          variant: 'error',
          error: query.error,
          action: <ChromeButton onClick={() => void query.refetch()}>{t('list.refresh')}</ChromeButton>,
        }
      : sources.length === 0
        ? {
            variant: 'empty',
            title: t(`usage.empty.${dimension}`),
            description: t('usage.empty.hint'),
          }
        : filtered.length === 0
          ? {
              variant: 'empty',
              compact: true,
              title: t('usage.no_match'),
              action: <ClearFiltersButton onClear={() => setSearch('')} />,
            }
          : null;

  const covered =
    data?.meteringStartedAt != null &&
    data.meteringStartedAt <= data.windowStart;

  const withBudget = selectedRows.filter((row) => row.budget);
  const takeBudget = selectedRows.filter((row) => row.budgetable);

  return (
    <>
      <ListPage
        appearance="surface"
        title={t('usage.title')}
        subtitle={t('usage.subtitle')}
        toolbar={
          <>
            <TableExportButton name={`molesignal-intake-usage-${dimension}`} columns={columns} rows={sortedRows} size="md" />
            <ChromeButton onClick={() => void refresh()} disabled={query.isFetching}>
              <RefreshCw className="h-3 w-3" /> {t('list.refresh')}
            </ChromeButton>
          </>
        }
        state={state}
        listHead={
          <>
            <ViewTabs<UsageDimension>
              label={t('usage.dimensions_label')}
              value={dimension}
              onValueChange={changeDimension}
              items={USAGE_DIMENSIONS.map((key) => ({ key, label: t(`usage.dimensions.${key}`) }))}
            />
            <QueryInput
              value={search}
              onChange={setSearch}
              placeholder={t('usage.search_placeholder') ?? ''}
              className="h-8 min-w-[180px] max-w-[280px] flex-1"
            />
            <FittedSelect
              value={usageWindow.key}
              onValueChange={(next) => setParam('window', next === '24h' ? null : next)}
              maxWidth={160}
              aria-label={t('usage.window_label')}
              options={USAGE_WINDOWS.map(({ key }) => ({ value: key, label: t(`usage.windows.${key}`) }))}
            />
            <div
              data-testid="usage-status"
              className="ml-auto flex items-baseline gap-x-2 whitespace-nowrap font-sans text-xs text-tx-2"
            >
              {data && !data.available ? (
                <span className="text-orange">{t('usage.unavailable')}</span>
              ) : data && data.meteringStartedAt == null ? (
                <span className="text-tx-3">{t('usage.not_metered')}</span>
              ) : data ? (
                <>
                  <span>
                    {t('usage.total', {
                      bytes: bytesFigure(data.totalBytes, covered),
                      events: countFigure(data.totalEvents, covered),
                    })}
                  </span>
                  {!covered && data.meteringStartedAt != null && (
                    <span className="text-tx-3">
                      {t('usage.metering_since', { time: formatter.micros(data.meteringStartedAt, false) })}
                    </span>
                  )}
                  {data.truncated && <span className="text-tx-3">{t('usage.truncated')}</span>}
                </>
              ) : null}
            </div>
          </>
        }
      >
        {selectedRows.length > 0 && (
          <TableBulkBar count={selectedRows.length} onClear={clearSelection} data-testid="usage-bulk">
            <ChromeButton size="sm" disabled={takeBudget.length === 0} onClick={() => setBudgeting(takeBudget)}>
              {t('usage.budget.set')}
            </ChromeButton>
            <ChromeButton size="sm" disabled={withBudget.length === 0} onClick={() => setRemoving(withBudget)}>
              <Trash2 className="h-3 w-3" />
              {t('usage.budget.remove')}
            </ChromeButton>
          </TableBulkBar>
        )}
        <DataTable
          className="rounded-none border-0 bg-transparent"
          rows={pagination.pageItems}
          rowKey={(row) => row.sourceKey}
          columns={columns}
          sort={sort}
          onSortChange={setSort}
          onRowClick={setOpened}
          {...(canManage ? { selection } : {})}
        />
        <StreamPagination {...pagination} />
      </ListPage>

      <SourceDrawer
        dimension={dimension}
        usageWindow={usageWindow}
        source={openedRow}
        canManage={canManage}
        onClose={() => setOpened(null)}
        onSetBudget={(row) => setBudgeting([row])}
      />

      <BudgetDrawer
        dimension={dimension}
        sources={budgeting}
        onClose={() => setBudgeting([])}
        onSaved={(allSaved) => {
          if (allSaved) clearSelection();
          return refresh();
        }}
      />

      <ConfirmDialog
        open={removing.length > 0}
        title={t('usage.budget.remove_title', { count: removing.length })}
        description={t('usage.budget.remove_description')}
        confirmLabel={t('usage.budget.remove')}
        destructive
        onOpenChange={(open) => {
          if (!open) setRemoving([]);
        }}
        onConfirm={() => remove.mutate(removing)}
      />
    </>
  );
}
