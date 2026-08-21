import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { FileUp, Plus } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate, useParams } from 'react-router-dom';

import { ConfirmDialog } from '@/admin';
import * as extendTablesApi from '@/api/extendTables';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { ChromeButton } from '@/shell/chrome';
import { EmptyState } from '@/shell/EmptyState';
import { PageBody, PageHeader } from '@/shell/PageHeader';
import { toast } from '@/shell/ui/sonner';

import {
  SchemaPanel,
  SettingsPanel,
  UsagePanel,
} from './detail/InformationPanels';
import {
  DetailSkeleton,
  DetailTabs,
  TableMetadata,
} from './detail/Metadata';
import { RecordsPanel } from './detail/RecordsPanel';
import { SchemaEditorDrawer } from './detail/SchemaEditorDrawer';
import type { DetailTab } from './detail/types';
import {
  ImportExtendRowsDrawer,
  UpsertExtendRowDrawer,
} from './Drawers';
import { inferValueFields } from './model';

export function ExtendTableDetail() {
  const { table: tableParam = '' } = useParams<{ table: string }>();
  const tableName = decodeURIComponent(tableParam);
  const { t } = useTranslation('functions');
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const editAccess = useActionAccess({ permission: 'functions.edit' });
  const deleteAccess = useActionAccess({ permission: 'functions.delete' });
  const [tab, setTab] = React.useState<DetailTab>('records');
  const [search, setSearch] = React.useState('');
  const [editingRow, setEditingRow] =
    React.useState<extendTablesApi.ExtendRow | null>(null);
  const [addOpen, setAddOpen] = React.useState(false);
  const [importOpen, setImportOpen] = React.useState(false);
  const [schemaEditorOpen, setSchemaEditorOpen] = React.useState(false);
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
        queryClient.invalidateQueries({
          queryKey: ['extend-tables', 'rows', tableName],
        }),
        queryClient.invalidateQueries({ queryKey: ['extend-tables', 'list'] }),
      ]);
      toast.success(t('extend_tables.toast_deleted'));
      setPendingDeleteRow(null);
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const removeTable = useMutation({
    mutationFn: () => extendTablesApi.deleteTable(tableName),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: ['extend-tables'] });
      toast.success(t('extend_tables.toast_table_deleted'));
      navigate('/extend-tables');
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const updateTableFields = useMutation({
    mutationFn: (valueFields: extendTablesApi.ExtendValueField[]) =>
      extendTablesApi.updateTableFields(tableName, valueFields),
    onSuccess: (updatedTable) => {
      queryClient.setQueryData<extendTablesApi.ExtendTableSummary[]>(
        ['extend-tables', 'list'],
        (current) =>
          current?.map((candidate) =>
            candidate.table_name === updatedTable.table_name
              ? updatedTable
              : candidate,
          ),
      );
      setSchemaEditorOpen(false);
      toast.success(t('extend_tables.toast_schema_updated'));
      void queryClient.invalidateQueries({
        queryKey: ['extend-tables', 'list'],
      });
    },
    onError: (error) => {
      toast.error(toApiError(error).message);
      void Promise.all([
        queryClient.invalidateQueries({
          queryKey: ['extend-tables', 'rows', tableName],
        }),
        queryClient.invalidateQueries({ queryKey: ['extend-tables', 'list'] }),
      ]);
    },
  });

  const loading = tablesQuery.isLoading || rowsQuery.isLoading;
  const error = tablesQuery.error ?? rowsQuery.error;

  return (
    <>
      <PageHeader
        title={tableName || t('extend_tables.title')}
        subtitle={table?.description || t('extend_tables.detail_subtitle')}
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
      <PageBody className="space-y-[12px]">
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
            <DetailTabs
              value={tab}
              table={table}
              fieldCount={fields.length}
              onChange={setTab}
            />
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
              <SchemaPanel
                editAccess={editAccess}
                table={table}
                fields={fields}
                onEdit={() => setSchemaEditorOpen(true)}
              />
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
          <SchemaEditorDrawer
            access={editAccess}
            open={schemaEditorOpen}
            table={table}
            fields={fields}
            busy={updateTableFields.isPending}
            onClose={() => setSchemaEditorOpen(false)}
            onSave={(nextFields) => updateTableFields.mutate(nextFields)}
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
