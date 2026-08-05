import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  ArrowRight,
  CircleCheck,
  CircleDashed,
  Database,
  MoreHorizontal,
  Plus,
  Search,
  TableProperties,
  Trash2,
  Workflow,
} from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link, useNavigate } from 'react-router-dom';

import { ConfirmDialog } from '@/admin';
import * as extendTablesApi from '@/api/extendTables';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { type ProductStateProps } from '@/product/states';
import { ListPage } from '@/product/templates';
import { ChromeButton } from '@/shell/chrome';
import { EmptyState } from '@/shell/EmptyState';
import { FormSelect } from '@/shell/FormDrawer';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/shell/ui/dropdown-menu';
import { toast } from '@/shell/ui/sonner';

import { CreateExtendTableDrawer } from './Drawers';
import { formatRelativeMicros } from '../../pipelines/presentation';

type StatusFilter = 'all' | 'healthy' | 'empty';
type UsageFilter = 'all' | 'used' | 'unused';

export function ExtendTables() {
  const { t, i18n } = useTranslation('functions');
  const navigate = useNavigate();
  const qc = useQueryClient();
  const createAccess = useActionAccess({
    permission: 'functions.create',
  });
  const deleteAccess = useActionAccess({
    permission: 'functions.delete',
  });
  const [search, setSearch] = React.useState('');
  const [statusFilter, setStatusFilter] = React.useState<StatusFilter>('all');
  const [usageFilter, setUsageFilter] = React.useState<UsageFilter>('all');
  const [createOpen, setCreateOpen] = React.useState(false);
  const [pendingDelete, setPendingDelete] =
    React.useState<extendTablesApi.ExtendTableSummary | null>(null);

  const tablesQuery = useQuery({
    queryKey: ['extend-tables', 'list'],
    queryFn: () => extendTablesApi.listTables(),
  });

  const allTables = React.useMemo(
    () => tablesQuery.data ?? [],
    [tablesQuery.data],
  );
  const tables = React.useMemo(() => {
    const needle = search.trim().toLocaleLowerCase();
    return allTables.filter((table) => {
      const matchesSearch =
        !needle ||
        table.table_name.toLocaleLowerCase().includes(needle) ||
        table.description.toLocaleLowerCase().includes(needle) ||
        table.value_fields.some((field) =>
          field.name.toLocaleLowerCase().includes(needle),
        );
      const matchesStatus =
        statusFilter === 'all' ||
        (statusFilter === 'healthy' && table.row_count > 0) ||
        (statusFilter === 'empty' && table.row_count === 0);
      const matchesUsage =
        usageFilter === 'all' ||
        (usageFilter === 'used' && table.usage_locations.length > 0) ||
        (usageFilter === 'unused' && table.usage_locations.length === 0);
      return matchesSearch && matchesStatus && matchesUsage;
    });
  }, [allTables, search, statusFilter, usageFilter]);

  const deleteTable = useMutation({
    mutationFn: (table: string) => extendTablesApi.deleteTable(table),
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: ['extend-tables'] });
      toast.success(t('extend_tables.toast_table_deleted'));
      setPendingDelete(null);
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  let listState: ProductStateProps | null = null;
  if (tablesQuery.isLoading) {
    listState = { variant: 'loading' };
  } else if (tablesQuery.isError) {
    listState = { variant: 'error', error: tablesQuery.error };
  } else if (allTables.length === 0) {
    listState = {
      variant: 'empty',
      title: t('extend_tables.empty_title'),
      description: t('extend_tables.empty_description'),
      action: (
        <ChromeButton
          variant="primary"
          disabled={createAccess.disabled}
          disabledReason={createAccess.reason}
          onClick={() => createAccess.allowed && setCreateOpen(true)}
        >
          <Plus className="h-3.5 w-3.5" />
          {t('extend_tables.new_table')}
        </ChromeButton>
      ),
    };
  }

  const rowCount = allTables.reduce((total, table) => total + table.row_count, 0);

  return (
    <>
      <ListPage
        title={t('extend_tables.title')}
        subtitle={t('extend_tables.subtitle')}
        toolbar={
          <ChromeButton
            variant="primary"
            disabled={createAccess.disabled}
            disabledReason={createAccess.reason}
            onClick={() => createAccess.allowed && setCreateOpen(true)}
          >
            <Plus className="h-3.5 w-3.5" />
            {t('extend_tables.new_table')}
          </ChromeButton>
        }
        filters={
          <div className="flex w-full flex-wrap items-center gap-2">
            <label className="relative min-w-[240px] flex-1 sm:max-w-sm">
              <Search className="pointer-events-none absolute left-3 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-tx-3" />
              <input
                value={search}
                onChange={(event) => setSearch(event.target.value)}
                placeholder={t('extend_tables.search_placeholder')}
                className="h-9 w-full rounded-md border border-bd-1 bg-bg-2 pl-9 pr-3 text-sm text-tx-0 placeholder:text-tx-3 focus:outline-none focus:ring-2 focus:ring-indigo/20"
              />
            </label>
            <FormSelect
              value={statusFilter}
              onChange={(value) => setStatusFilter(value as StatusFilter)}
              className="w-36"
              options={[
                { value: 'all', label: t('extend_tables.filters.all_status') },
                { value: 'healthy', label: t('extend_tables.filters.healthy') },
                { value: 'empty', label: t('extend_tables.filters.empty') },
              ]}
            />
            <FormSelect
              value={usageFilter}
              onChange={(value) => setUsageFilter(value as UsageFilter)}
              className="w-40"
              options={[
                { value: 'all', label: t('extend_tables.filters.all_usage') },
                { value: 'used', label: t('extend_tables.filters.used') },
                { value: 'unused', label: t('extend_tables.filters.unused') },
              ]}
            />
          </div>
        }
        actionBar={
          <div className="flex w-full items-center gap-2 text-xs text-tx-2">
            <Database className="h-3.5 w-3.5 text-tx-3" />
            <span>
              {t('extend_tables.summary', {
                tables: allTables.length,
                rows: rowCount.toLocaleString(i18n.language),
              })}
            </span>
          </div>
        }
        state={listState}
      >
        {tables.length === 0 ? (
          <div className="rounded-lg border border-bd-0 bg-bg-1">
            <EmptyState
              strategy="query-first"
              title={t('extend_tables.no_match_title')}
              description={t('extend_tables.no_match_description')}
              className="min-h-60"
            />
          </div>
        ) : (
          <div className="overflow-hidden rounded-lg border border-bd-0 bg-bg-1">
            <div className="hidden min-h-10 grid-cols-[minmax(280px,1.7fr)_minmax(220px,1.3fr)_120px_150px_150px_136px] items-center gap-4 border-b border-bd-0 bg-bg-2 px-5 text-type-micro font-strong uppercase tracking-wider text-tx-3 xl:grid">
              <span>{t('extend_tables.columns.name')}</span>
              <span>{t('extend_tables.columns.schema')}</span>
              <span>{t('extend_tables.columns.rows')}</span>
              <span>{t('extend_tables.columns.usage')}</span>
              <span>{t('extend_tables.columns.updated')}</span>
              <span data-testid="extend-table-status-header" className="text-center">
                {t('extend_tables.columns.status')}
              </span>
            </div>
            {tables.map((table) => (
              <div
                key={table.table_name}
                className="group grid min-h-[76px] gap-3 border-b border-bd-0 px-5 py-4 transition-colors last:border-b-0 hover:bg-bg-2 xl:grid-cols-[minmax(280px,1.7fr)_minmax(220px,1.3fr)_120px_150px_150px_136px] xl:items-center xl:gap-4"
              >
                <Link
                  to={`/extend-tables/${encodeURIComponent(table.table_name)}`}
                  className="min-w-0 rounded focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo"
                >
                  <div className="flex items-center gap-2">
                    <TableProperties className="h-4 w-4 shrink-0 text-indigo-soft" />
                    <span className="truncate text-sm font-display-strong text-tx-0 group-hover:text-indigo-soft">
                      {table.table_name}
                    </span>
                  </div>
                  <p className="mt-1 truncate pl-6 text-xs text-tx-2">
                    {table.description || t('extend_tables.no_description')}
                  </p>
                </Link>
                <div className="min-w-0 pl-6 xl:pl-0">
                  <div className="truncate font-mono text-xs text-tx-1">
                    <span className="text-indigo-soft">{table.key_field}</span>
                    <ArrowRight className="mx-1.5 inline h-3 w-3 text-tx-3" />
                    <span>
                      {table.value_fields.length > 0
                        ? table.value_fields
                            .slice(0, 4)
                            .map((field) => field.name)
                            .join(', ')
                        : t('extend_tables.flexible_fields')}
                    </span>
                  </div>
                  {table.value_fields.length > 4 && (
                    <span className="mt-1 block text-type-micro text-tx-3">
                      {t('extend_tables.more_fields', {
                        count: table.value_fields.length - 4,
                      })}
                    </span>
                  )}
                </div>
                <Metric
                  label={t('extend_tables.columns.rows')}
                  value={table.row_count.toLocaleString(i18n.language)}
                />
                <Metric
                  label={t('extend_tables.columns.usage')}
                  value={
                    table.usage_locations.length > 0 ? (
                      <span className="inline-flex items-center gap-1.5">
                        <Workflow className="h-3.5 w-3.5 text-purple-soft" />
                        {t('extend_tables.usage_count', {
                          count: table.usage_locations.length,
                        })}
                      </span>
                    ) : (
                      t('extend_tables.not_used')
                    )
                  }
                />
                <Metric
                  label={t('extend_tables.columns.updated')}
                  value={formatRelativeMicros(
                    table.updated_at,
                    i18n.language,
                  )}
                />
                <div
                  data-testid="extend-table-status-cell"
                  className="relative flex items-center justify-between gap-2 xl:justify-center"
                >
                  <span className="xl:hidden text-type-micro uppercase tracking-wider text-tx-3">
                    {t('extend_tables.columns.status')}
                  </span>
                  <TableStatus populated={table.row_count > 0} />
                  <DropdownMenu>
                    <DropdownMenuTrigger asChild>
                      <button
                        type="button"
                        className="grid h-8 w-8 shrink-0 place-items-center rounded-md text-tx-3 opacity-100 hover:bg-bg-3 hover:text-tx-0 xl:pointer-events-none xl:absolute xl:right-0 xl:top-1/2 xl:-translate-y-1/2 xl:opacity-0 xl:group-hover:pointer-events-auto xl:group-hover:opacity-100"
                        aria-label={t('extend_tables.table_actions', {
                          table: table.table_name,
                        })}
                      >
                        <MoreHorizontal className="h-4 w-4" />
                      </button>
                    </DropdownMenuTrigger>
                    <DropdownMenuContent align="end">
                      <DropdownMenuItem
                        onSelect={() =>
                          navigate(
                            `/extend-tables/${encodeURIComponent(table.table_name)}`,
                          )
                        }
                      >
                        <ArrowRight className="mr-2 h-3.5 w-3.5" />
                        {t('extend_tables.view_table')}
                      </DropdownMenuItem>
                      <DropdownMenuSeparator />
                      <DropdownMenuItem
                        disabled={deleteAccess.disabled}
                        disabledReason={deleteAccess.reason}
                        className="text-red focus:text-red"
                        onSelect={() =>
                          deleteAccess.allowed && setPendingDelete(table)
                        }
                      >
                        <Trash2 className="mr-2 h-3.5 w-3.5" />
                        {t('extend_tables.delete_table')}
                      </DropdownMenuItem>
                    </DropdownMenuContent>
                  </DropdownMenu>
                </div>
              </div>
            ))}
          </div>
        )}
      </ListPage>

      <CreateExtendTableDrawer
        access={createAccess}
        open={createOpen}
        onClose={() => setCreateOpen(false)}
        onCreated={(table) =>
          navigate(`/extend-tables/${encodeURIComponent(table.table_name)}`)
        }
      />
      <ConfirmDialog
        open={pendingDelete !== null}
        onOpenChange={(open) => !open && setPendingDelete(null)}
        title={t('extend_tables.delete_table_title')}
        description={t('extend_tables.delete_table_description', {
          table: pendingDelete?.table_name,
          count: pendingDelete?.row_count ?? 0,
        })}
        confirmLabel={t('extend_tables.confirm_delete')}
        cancelLabel={t('extend_tables.cancel')}
        destructive
        busy={deleteTable.isPending}
        disabled={deleteAccess.disabled}
        disabledReason={deleteAccess.reason}
        onConfirm={() => {
          if (deleteAccess.allowed && pendingDelete) {
            deleteTable.mutate(pendingDelete.table_name);
          }
        }}
      />
    </>
  );
}

function Metric({
  label,
  value,
}: {
  label: string;
  value: React.ReactNode;
}) {
  return (
    <div className="flex min-w-0 items-center justify-between gap-3 pl-6 text-xs text-tx-1 xl:block xl:pl-0">
      <span className="text-type-micro uppercase tracking-wider text-tx-3 xl:hidden">
        {label}
      </span>
      <span className="min-w-0 truncate">{value}</span>
    </div>
  );
}

function TableStatus({ populated }: { populated: boolean }) {
  const { t } = useTranslation('functions');
  return (
    <span
      className={
        populated
          ? 'inline-flex items-center gap-1.5 rounded-full bg-green-dim px-2 py-1 text-type-micro font-strong text-green'
          : 'inline-flex items-center gap-1.5 rounded-full bg-bg-3 px-2 py-1 text-type-micro font-strong text-tx-2'
      }
    >
      {populated ? (
        <CircleCheck className="h-3 w-3" />
      ) : (
        <CircleDashed className="h-3 w-3" />
      )}
      {populated
        ? t('extend_tables.status.healthy')
        : t('extend_tables.status.empty')}
    </span>
  );
}
