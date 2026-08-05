import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  Braces,
  Clock3,
  Code2,
  Edit3,
  FileUp,
  KeyRound,
  type LucideIcon,
  MoreHorizontal,
  Plus,
  Rows3,
  Search,
  Settings2,
  Trash2,
  Workflow,
} from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link, useNavigate, useParams } from 'react-router-dom';

import { ConfirmDialog } from '@/admin';
import * as extendTablesApi from '@/api/extendTables';
import { toApiError } from '@/lib/http';
import {
  type ActionAccess,
  useActionAccess,
} from '@/product/actionAccess';
import { ChromeButton } from '@/shell/chrome';
import { DisabledControl } from '@/shell/DisabledControl';
import { EmptyState } from '@/shell/EmptyState';
import { cn } from '@/shell/lib/cn';
import { PageBody, PageHeader } from '@/shell/PageHeader';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/shell/ui/dropdown-menu';
import { toast } from '@/shell/ui/sonner';
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/shell/ui/table';
import { Tabs, TabsList, TabsTrigger } from '@/shell/ui/tabs';

import {
  ImportExtendRowsDrawer,
  UpsertExtendRowDrawer,
} from './Drawers';
import {
  displayValue,
  inferValueFields,
  valueAsObject,
} from './model';
import { formatRelativeMicros } from '../../pipelines/presentation';

type DetailTab = 'records' | 'schema' | 'usage' | 'settings';

export function ExtendTableDetail() {
  const { table: tableParam = '' } = useParams<{ table: string }>();
  const tableName = decodeURIComponent(tableParam);
  const { t } = useTranslation('functions');
  const navigate = useNavigate();
  const qc = useQueryClient();
  const editAccess = useActionAccess({
    permission: 'functions.edit',
  });
  const deleteAccess = useActionAccess({
    permission: 'functions.delete',
  });
  const [tab, setTab] = React.useState<DetailTab>('records');
  const [search, setSearch] = React.useState('');
  const [editingRow, setEditingRow] =
    React.useState<extendTablesApi.ExtendRow | null>(null);
  const [addOpen, setAddOpen] = React.useState(false);
  const [importOpen, setImportOpen] = React.useState(false);
  const [expandedKey, setExpandedKey] = React.useState<string | null>(null);
  const [pendingDeleteRow, setPendingDeleteRow] =
    React.useState<extendTablesApi.ExtendRow | null>(null);
  const [deleteTableOpen, setDeleteTableOpen] = React.useState(false);

  const tablesQuery = useQuery({
    queryKey: ['extend-tables', 'list'],
    queryFn: () => extendTablesApi.listTables(),
  });
  const rowsQuery = useQuery({
    queryKey: ['extend-tables', 'rows', tableName],
    queryFn: () => extendTablesApi.listRows(tableName),
    enabled: Boolean(tableName),
  });

  const table = tablesQuery.data?.find(
    (candidate) => candidate.table_name === tableName,
  );
  const rows = React.useMemo(() => rowsQuery.data ?? [], [rowsQuery.data]);
  const fields = React.useMemo(
    () =>
      table?.value_fields.length
        ? table.value_fields
        : inferValueFields(rows),
    [rows, table?.value_fields],
  );
  const visibleFields = fields.slice(0, 6);
  const filteredRows = React.useMemo(() => {
    const needle = search.trim().toLocaleLowerCase();
    if (!needle) return rows;
    return rows.filter(
      (row) =>
        row.key.toLocaleLowerCase().includes(needle) ||
        JSON.stringify(row.value_json).toLocaleLowerCase().includes(needle),
    );
  }, [rows, search]);

  const removeRow = useMutation({
    mutationFn: (row: extendTablesApi.ExtendRow) =>
      extendTablesApi.remove(tableName, row.key),
    onSuccess: async () => {
      await Promise.all([
        qc.invalidateQueries({
          queryKey: ['extend-tables', 'rows', tableName],
        }),
        qc.invalidateQueries({ queryKey: ['extend-tables', 'list'] }),
      ]);
      toast.success(t('extend_tables.toast_deleted'));
      setPendingDeleteRow(null);
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const removeTable = useMutation({
    mutationFn: () => extendTablesApi.deleteTable(tableName),
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: ['extend-tables'] });
      toast.success(t('extend_tables.toast_table_deleted'));
      navigate('/extend-tables');
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const loading = tablesQuery.isLoading || rowsQuery.isLoading;
  const error = tablesQuery.error ?? rowsQuery.error;

  return (
    <>
      <PageHeader
        title={tableName || t('extend_tables.title')}
        subtitle={table?.description || t('extend_tables.detail_subtitle')}
        backTo="/extend-tables"
        breadcrumbs={[
          {
            labelKey: 'extend_tables',
            label: t('extend_tables.title'),
            to: '/extend-tables',
          },
          {
            labelKey: 'extend_table_detail',
            label: tableName,
          },
        ]}
        toolbar={
          table ? (
            <>
              <ChromeButton
                disabled={editAccess.disabled}
                disabledReason={editAccess.reason}
                onClick={() => editAccess.allowed && setImportOpen(true)}
              >
                <FileUp className="h-3.5 w-3.5" />
                {t('extend_tables.import_data')}
              </ChromeButton>
              <ChromeButton
                variant="primary"
                disabled={editAccess.disabled}
                disabledReason={editAccess.reason}
                onClick={() => editAccess.allowed && setAddOpen(true)}
              >
                <Plus className="h-3.5 w-3.5" />
                {t('extend_tables.add_record')}
              </ChromeButton>
            </>
          ) : undefined
        }
      />
      <PageBody className="space-y-5">
        {loading ? (
          <DetailSkeleton />
        ) : error ? (
          <EmptyState
            strategy="backend-pending"
            title={t('extend_tables.load_error')}
            description={toApiError(error).message}
          />
        ) : !table ? (
          <EmptyState
            strategy="query-first"
            title={t('extend_tables.not_found_title')}
            description={t('extend_tables.not_found_description', {
              table: tableName,
            })}
            primaryAction={{
              label: t('extend_tables.back_to_tables'),
              to: '/extend-tables',
            }}
          />
        ) : (
          <>
            <TableMetadata table={table} />
            <div className="border-b border-bd-0">
              <Tabs
                value={tab}
                onValueChange={(value) => setTab(value as DetailTab)}
              >
                <TabsList className="h-10 max-w-full justify-start overflow-x-auto rounded-none bg-transparent p-0">
                  <DetailTabTrigger
                    value="records"
                    icon={Rows3}
                    label={t('extend_tables.tabs.records')}
                    count={table.row_count}
                  />
                  <DetailTabTrigger
                    value="schema"
                    icon={Braces}
                    label={t('extend_tables.tabs.schema')}
                    count={fields.length + 1}
                  />
                  <DetailTabTrigger
                    value="usage"
                    icon={Workflow}
                    label={t('extend_tables.tabs.usage')}
                    count={table.usage_locations.length}
                  />
                  <DetailTabTrigger
                    value="settings"
                    icon={Settings2}
                    label={t('extend_tables.tabs.settings')}
                  />
                </TabsList>
              </Tabs>
            </div>

            {tab === 'records' && (
              <RecordsPanel
                editAccess={editAccess}
                table={table}
                rows={filteredRows}
                allRows={rows}
                fields={visibleFields}
                totalFieldCount={fields.length}
                search={search}
                onSearchChange={setSearch}
                expandedKey={expandedKey}
                onExpandedKeyChange={setExpandedKey}
                onEdit={setEditingRow}
                onDelete={setPendingDeleteRow}
                onAdd={() => setAddOpen(true)}
                onImport={() => setImportOpen(true)}
              />
            )}
            {tab === 'schema' && (
              <SchemaPanel table={table} fields={fields} />
            )}
            {tab === 'usage' && <UsagePanel table={table} />}
            {tab === 'settings' && (
              <SettingsPanel
                deleteAccess={deleteAccess}
                table={table}
                onDelete={() => setDeleteTableOpen(true)}
              />
            )}
          </>
        )}
      </PageBody>

      {table && (
        <>
          <UpsertExtendRowDrawer
            access={editAccess}
            open={addOpen || editingRow !== null}
            table={{ ...table, value_fields: fields }}
            rows={rows}
            editingRow={editingRow}
            onClose={() => {
              setAddOpen(false);
              setEditingRow(null);
            }}
          />
          <ImportExtendRowsDrawer
            access={editAccess}
            open={importOpen}
            table={table}
            rows={rows}
            onClose={() => setImportOpen(false)}
          />
        </>
      )}
      <ConfirmDialog
        open={pendingDeleteRow !== null}
        onOpenChange={(open) => !open && setPendingDeleteRow(null)}
        title={t('extend_tables.delete_record_title')}
        description={t('extend_tables.delete_record_description', {
          key: pendingDeleteRow?.key,
        })}
        confirmLabel={t('extend_tables.confirm_delete')}
        cancelLabel={t('extend_tables.cancel')}
        destructive
        busy={removeRow.isPending}
        disabled={editAccess.disabled}
        disabledReason={editAccess.reason}
        onConfirm={() => {
          if (editAccess.allowed && pendingDeleteRow) {
            removeRow.mutate(pendingDeleteRow);
          }
        }}
      />
      <ConfirmDialog
        open={deleteTableOpen}
        onOpenChange={setDeleteTableOpen}
        title={t('extend_tables.delete_table_title')}
        description={t('extend_tables.delete_table_description', {
          table: tableName,
          count: table?.row_count ?? 0,
        })}
        confirmLabel={t('extend_tables.confirm_delete')}
        cancelLabel={t('extend_tables.cancel')}
        destructive
        busy={removeTable.isPending}
        disabled={deleteAccess.disabled}
        disabledReason={deleteAccess.reason}
        onConfirm={() => deleteAccess.allowed && removeTable.mutate()}
      />
    </>
  );
}

function TableMetadata({
  table,
}: {
  table: extendTablesApi.ExtendTableSummary;
}) {
  const { t, i18n } = useTranslation('functions');
  const items = [
    {
      icon: KeyRound,
      label: t('extend_tables.key_field'),
      value: table.key_field,
      mono: true,
    },
    {
      icon: Braces,
      label: t('extend_tables.value_fields'),
      value:
        table.value_fields.length > 0
          ? t('extend_tables.field_count', { count: table.value_fields.length })
          : t('extend_tables.flexible_fields'),
    },
    {
      icon: Rows3,
      label: t('extend_tables.record_count'),
      value: table.row_count.toLocaleString(i18n.language),
    },
    {
      icon: Clock3,
      label: t('extend_tables.columns.updated'),
      value: formatRelativeMicros(table.updated_at, i18n.language),
    },
  ];
  return (
    <div className="grid overflow-hidden rounded-lg border border-bd-0 bg-bg-1 sm:grid-cols-2 xl:grid-cols-4">
      {items.map((item) => (
        <div
          key={item.label}
          className="flex min-h-20 items-center gap-3 border-b border-bd-0 px-5 py-4 last:border-b-0 sm:border-r sm:[&:nth-child(2)]:border-r-0 sm:[&:nth-child(n+3)]:border-b-0 xl:border-b-0 xl:[&:nth-child(2)]:border-r xl:last:border-r-0"
        >
          <span className="grid h-9 w-9 place-items-center rounded-md bg-bg-2 text-tx-2">
            <item.icon className="h-4 w-4" />
          </span>
          <span className="min-w-0">
            <span className="block text-type-micro uppercase tracking-wider text-tx-3">
              {item.label}
            </span>
            <span
              className={cn(
                'mt-1 block truncate text-sm font-strong text-tx-0',
                item.mono && 'font-mono',
              )}
            >
              {item.value}
            </span>
          </span>
        </div>
      ))}
    </div>
  );
}

function RecordsPanel({
  editAccess,
  table,
  rows,
  allRows,
  fields,
  totalFieldCount,
  search,
  onSearchChange,
  expandedKey,
  onExpandedKeyChange,
  onEdit,
  onDelete,
  onAdd,
  onImport,
}: {
  editAccess: ActionAccess;
  table: extendTablesApi.ExtendTableSummary;
  rows: extendTablesApi.ExtendRow[];
  allRows: extendTablesApi.ExtendRow[];
  fields: extendTablesApi.ExtendValueField[];
  totalFieldCount: number;
  search: string;
  onSearchChange: (value: string) => void;
  expandedKey: string | null;
  onExpandedKeyChange: (value: string | null) => void;
  onEdit: (row: extendTablesApi.ExtendRow) => void;
  onDelete: (row: extendTablesApi.ExtendRow) => void;
  onAdd: () => void;
  onImport: () => void;
}) {
  const { t, i18n } = useTranslation('functions');
  return (
    <section className="overflow-hidden rounded-lg border border-bd-0 bg-bg-1">
      <div className="flex flex-wrap items-center gap-3 border-b border-bd-0 px-4 py-3">
        <label className="relative min-w-[240px] flex-1 sm:max-w-sm">
          <Search className="pointer-events-none absolute left-3 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-tx-3" />
          <input
            value={search}
            onChange={(event) => onSearchChange(event.target.value)}
            placeholder={t('extend_tables.search_key_placeholder', {
              key: table.key_field,
            })}
            className="h-9 w-full rounded-md border border-bd-1 bg-bg-2 pl-9 pr-3 text-sm text-tx-0 placeholder:text-tx-3 focus:outline-none focus:ring-2 focus:ring-indigo/20"
          />
        </label>
        <span className="ml-auto text-xs text-tx-2">
          {t('extend_tables.records_visible', {
            visible: rows.length,
            total: allRows.length,
          })}
        </span>
        {totalFieldCount > fields.length && (
          <span className="rounded bg-bg-2 px-2 py-1 text-type-micro text-tx-2">
            {t('extend_tables.hidden_fields', {
              count: totalFieldCount - fields.length,
            })}
          </span>
        )}
      </div>
      {allRows.length === 0 ? (
        <EmptyState
          strategy="create-first"
          title={t('extend_tables.empty_rows')}
          description={t('extend_tables.empty_rows_description')}
          primaryAction={{
            label: t('extend_tables.add_record'),
            onClick: onAdd,
            disabled: editAccess.disabled,
            disabledReason: editAccess.reason,
          }}
          secondaryAction={{
            label: t('extend_tables.import_data'),
            onClick: onImport,
            disabled: editAccess.disabled,
            disabledReason: editAccess.reason,
          }}
          className="min-h-72"
        />
      ) : rows.length === 0 ? (
        <EmptyState
          strategy="query-first"
          title={t('extend_tables.no_records_match')}
          description={t('extend_tables.no_records_match_description')}
          className="min-h-64"
        />
      ) : (
        <Table className="min-w-[880px]">
          <TableHeader>
            <TableRow className="h-10 hover:bg-transparent">
              <TableHead className="min-w-[200px] normal-case tracking-normal">
                {table.key_field}
              </TableHead>
              {fields.map((field) => (
                <TableHead
                  key={field.name}
                  className="min-w-[140px] normal-case tracking-normal"
                >
                  {field.name}
                </TableHead>
              ))}
              <TableHead className="w-[150px] normal-case tracking-normal">
                {t('extend_tables.columns.updated')}
              </TableHead>
              <TableHead className="w-16 text-right">
                {t('extend_tables.columns.actions')}
              </TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {rows.map((row) => {
              const values = valueAsObject(row.value_json);
              const expanded = expandedKey === row.key;
              return (
                <React.Fragment key={row.key}>
                  <TableRow className="h-12">
                    <TableCell>
                      <DisabledControl
                        disabled={editAccess.disabled}
                        reason={editAccess.reason}
                      >
                        <button
                          type="button"
                          disabled={editAccess.disabled}
                          aria-disabled={editAccess.disabled || undefined}
                          onClick={() => editAccess.allowed && onEdit(row)}
                          className="max-w-[260px] truncate rounded font-mono text-xs font-strong text-indigo-soft enabled:hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo disabled:cursor-not-allowed disabled:text-tx-3"
                        >
                          {row.key}
                        </button>
                      </DisabledControl>
                    </TableCell>
                    {fields.map((field) => (
                      <TableCell key={field.name}>
                        <span
                          className={cn(
                            'block max-w-[240px] truncate text-xs text-tx-1',
                            field.field_type === 'object' && 'font-mono',
                          )}
                          title={displayValue(values[field.name])}
                        >
                          {displayValue(values[field.name])}
                        </span>
                      </TableCell>
                    ))}
                    <TableCell className="whitespace-nowrap text-xs text-tx-2">
                      {formatRelativeMicros(
                        row.updated_at_micros,
                        i18n.language,
                      )}
                    </TableCell>
                    <TableCell className="text-right">
                      <DropdownMenu>
                        <DropdownMenuTrigger asChild>
                          <button
                            type="button"
                            className="inline-grid h-8 w-8 place-items-center rounded-md text-tx-3 hover:bg-bg-3 hover:text-tx-0"
                            aria-label={t('extend_tables.row_actions', {
                              key: row.key,
                            })}
                          >
                            <MoreHorizontal className="h-4 w-4" />
                          </button>
                        </DropdownMenuTrigger>
                        <DropdownMenuContent align="end">
                          <DropdownMenuItem
                            disabled={editAccess.disabled}
                            disabledReason={editAccess.reason}
                            onSelect={() => editAccess.allowed && onEdit(row)}
                          >
                            <Edit3 className="mr-2 h-3.5 w-3.5" />
                            {t('extend_tables.edit_record')}
                          </DropdownMenuItem>
                          <DropdownMenuItem
                            onSelect={() =>
                              onExpandedKeyChange(expanded ? null : row.key)
                            }
                          >
                            <Code2 className="mr-2 h-3.5 w-3.5" />
                            {expanded
                              ? t('extend_tables.hide_json')
                              : t('extend_tables.view_json')}
                          </DropdownMenuItem>
                          <DropdownMenuSeparator />
                          <DropdownMenuItem
                            disabled={editAccess.disabled}
                            disabledReason={editAccess.reason}
                            className="text-red focus:text-red"
                            onSelect={() => editAccess.allowed && onDelete(row)}
                          >
                            <Trash2 className="mr-2 h-3.5 w-3.5" />
                            {t('extend_tables.delete_record')}
                          </DropdownMenuItem>
                        </DropdownMenuContent>
                      </DropdownMenu>
                    </TableCell>
                  </TableRow>
                  {expanded && (
                    <TableRow className="hover:bg-transparent">
                      <TableCell colSpan={fields.length + 3} className="bg-bg-2 p-4">
                        <pre className="max-h-72 overflow-auto whitespace-pre-wrap rounded-md border border-bd-0 bg-bg-0 p-4 font-mono text-xs leading-relaxed text-tx-1">
                          {JSON.stringify(row.value_json, null, 2)}
                        </pre>
                      </TableCell>
                    </TableRow>
                  )}
                </React.Fragment>
              );
            })}
          </TableBody>
        </Table>
      )}
    </section>
  );
}

function SchemaPanel({
  table,
  fields,
}: {
  table: extendTablesApi.ExtendTableSummary;
  fields: extendTablesApi.ExtendValueField[];
}) {
  const { t } = useTranslation('functions');
  const rows: Array<
    extendTablesApi.ExtendValueField & { keyField?: boolean }
  > = [
    {
      name: table.key_field,
      field_type: 'string',
      required: true,
      description: t('extend_tables.primary_key_description'),
      keyField: true,
    },
    ...fields,
  ];
  return (
    <section className="overflow-hidden rounded-lg border border-bd-0 bg-bg-1">
      <div className="border-b border-bd-0 px-5 py-4">
        <h2 className="text-sm font-display-strong text-tx-0">
          {t('extend_tables.schema_title')}
        </h2>
        <p className="mt-1 text-xs text-tx-2">
          {t('extend_tables.schema_description')}
        </p>
      </div>
      <Table>
        <TableHeader>
          <TableRow className="h-10 hover:bg-transparent">
            <TableHead>{t('extend_tables.field_name')}</TableHead>
            <TableHead className="w-36">
              {t('extend_tables.field_type')}
            </TableHead>
            <TableHead className="w-28">
              {t('extend_tables.required')}
            </TableHead>
            <TableHead>{t('extend_tables.field_description')}</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {rows.map((field) => (
            <TableRow key={field.name} className="h-12">
              <TableCell>
                <span className="inline-flex items-center gap-2 font-mono text-xs font-strong text-tx-0">
                  {field.keyField && <KeyRound className="h-3.5 w-3.5 text-indigo-soft" />}
                  {field.name}
                </span>
              </TableCell>
              <TableCell>
                <span className="rounded bg-bg-3 px-2 py-1 font-mono text-type-micro uppercase text-tx-1">
                  {t(`extend_tables.types.${field.field_type}`)}
                </span>
              </TableCell>
              <TableCell className="text-xs text-tx-1">
                {field.required
                  ? t('extend_tables.yes')
                  : t('extend_tables.no')}
              </TableCell>
              <TableCell className="text-xs text-tx-2">
                {field.description || t('extend_tables.no_field_description')}
              </TableCell>
            </TableRow>
          ))}
        </TableBody>
      </Table>
    </section>
  );
}

function UsagePanel({
  table,
}: {
  table: extendTablesApi.ExtendTableSummary;
}) {
  const { t } = useTranslation('functions');
  if (table.usage_locations.length === 0) {
    return (
      <section className="rounded-lg border border-bd-0 bg-bg-1">
        <EmptyState
          strategy="none"
          icon={Workflow}
          title={t('extend_tables.no_usage_title')}
          description={t('extend_tables.no_usage_description')}
          className="min-h-72"
        />
      </section>
    );
  }
  return (
    <section className="overflow-hidden rounded-lg border border-bd-0 bg-bg-1">
      <div className="border-b border-bd-0 px-5 py-4">
        <h2 className="text-sm font-display-strong text-tx-0">
          {t('extend_tables.usage_title')}
        </h2>
        <p className="mt-1 text-xs text-tx-2">
          {t('extend_tables.usage_description')}
        </p>
      </div>
      {table.usage_locations.map((usage) => {
        const UsageIcon = usage.kind === 'pipeline' ? Workflow : Code2;
        return (
          <Link
            key={`${usage.kind}-${usage.id}`}
            to={
              usage.kind === 'pipeline'
                ? `/pipelines/${usage.id}`
                : '/saved-views'
            }
            className="flex min-h-16 items-center gap-3 border-b border-bd-0 px-5 py-3 last:border-b-0 hover:bg-bg-2"
          >
            <span className="grid h-9 w-9 place-items-center rounded-md bg-purple-dim text-purple-soft">
              <UsageIcon className="h-4 w-4" />
            </span>
            <span className="min-w-0 flex-1">
              <span className="block truncate text-sm font-strong text-tx-0">
                {usage.name}
              </span>
              <span className="mt-0.5 block text-xs text-tx-3">
                {usage.kind === 'pipeline'
                  ? t('extend_tables.usage_pipeline')
                  : t('extend_tables.usage_saved_view')}
              </span>
            </span>
          </Link>
        );
      })}
    </section>
  );
}

function SettingsPanel({
  deleteAccess,
  table,
  onDelete,
}: {
  deleteAccess: ActionAccess;
  table: extendTablesApi.ExtendTableSummary;
  onDelete: () => void;
}) {
  const { t } = useTranslation('functions');
  return (
    <section className="max-w-3xl space-y-5">
      <div className="rounded-lg border border-bd-0 bg-bg-1 p-5">
        <h2 className="text-sm font-display-strong text-tx-0">
          {t('extend_tables.table_information')}
        </h2>
        <dl className="mt-4 grid gap-4 sm:grid-cols-2">
          <DefinitionTerm
            label={t('extend_tables.columns.name')}
            value={table.table_name}
            mono
          />
          <DefinitionTerm
            label={t('extend_tables.key_field')}
            value={table.key_field}
            mono
          />
          <DefinitionTerm
            label={t('extend_tables.field_description')}
            value={table.description || t('extend_tables.no_description')}
          />
          <DefinitionTerm
            label={t('extend_tables.columns.status')}
            value={
              table.row_count > 0
                ? t('extend_tables.status.healthy')
                : t('extend_tables.status.empty')
            }
          />
        </dl>
      </div>
      <div className="rounded-lg border border-red/25 bg-red-dim p-5">
        <h2 className="text-sm font-display-strong text-red">
          {t('extend_tables.danger_zone')}
        </h2>
        <p className="mt-2 max-w-2xl text-xs leading-relaxed text-tx-1">
          {t('extend_tables.danger_zone_description', {
            count: table.row_count,
          })}
        </p>
        <ChromeButton
          disabled={deleteAccess.disabled}
          disabledReason={deleteAccess.reason}
          className="mt-4 border-red/35 text-red enabled:hover:border-red enabled:hover:bg-red-dim enabled:hover:text-red"
          onClick={() => deleteAccess.allowed && onDelete()}
        >
          <Trash2 className="h-3.5 w-3.5" />
          {t('extend_tables.delete_table')}
        </ChromeButton>
      </div>
    </section>
  );
}

function DefinitionTerm({
  label,
  value,
  mono,
}: {
  label: string;
  value: string;
  mono?: boolean;
}) {
  return (
    <div>
      <dt className="text-type-micro uppercase tracking-wider text-tx-3">
        {label}
      </dt>
      <dd
        className={cn(
          'mt-1.5 text-sm text-tx-0',
          mono && 'font-mono text-xs',
        )}
      >
        {value}
      </dd>
    </div>
  );
}

function DetailTabTrigger({
  value,
  icon: Icon,
  label,
  count,
}: {
  value: DetailTab;
  icon: LucideIcon;
  label: string;
  count?: number;
}) {
  return (
    <TabsTrigger
      value={value}
      className="h-10 gap-2 rounded-none border-b-2 border-transparent bg-transparent px-4 text-xs text-tx-2 shadow-none data-[state=active]:border-indigo data-[state=active]:bg-transparent data-[state=active]:text-tx-0 data-[state=active]:shadow-none"
    >
      <Icon className="h-3.5 w-3.5" />
      {label}
      {count !== undefined && (
        <span className="rounded-full bg-bg-3 px-1.5 py-0.5 font-mono text-type-micro text-tx-2">
          {count}
        </span>
      )}
    </TabsTrigger>
  );
}

function DetailSkeleton() {
  return (
    <div className="animate-pulse space-y-5">
      <div className="h-20 rounded-lg bg-bg-2" />
      <div className="h-10 w-96 rounded bg-bg-2" />
      <div className="h-80 rounded-lg bg-bg-2" />
    </div>
  );
}
