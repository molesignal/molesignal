import { Code2, Edit3, MoreHorizontal, Search, Trash2 } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import type {
  ExtendRow,
  ExtendTableSummary,
  ExtendValueField,
} from '@/api/extendTables';
import type { ActionAccess } from '@/product/actionAccess';
import { DisabledControl } from '@/shell/DisabledControl';
import { EmptyState } from '@/shell/EmptyState';
import { cn } from '@/shell/lib/cn';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/shell/ui/dropdown-menu';
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/shell/ui/table';

import { formatRelativeMicros } from '../../../pipelines/presentation';
import { displayValue, valueAsObject } from '../model';

export function RecordsPanel({
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
  table: ExtendTableSummary;
  rows: ExtendRow[];
  allRows: ExtendRow[];
  fields: ExtendValueField[];
  totalFieldCount: number;
  search: string;
  onSearchChange: (value: string) => void;
  expandedKey: string | null;
  onExpandedKeyChange: (value: string | null) => void;
  onEdit: (row: ExtendRow) => void;
  onDelete: (row: ExtendRow) => void;
  onAdd: () => void;
  onImport: () => void;
}) {
  const { t, i18n } = useTranslation('functions');
  return (
    <section data-extend-table-records className="min-w-0">
      <div className="flex flex-wrap items-center gap-3 border-b border-bd-0 pb-3">
        <label className="relative w-full min-w-0 flex-1 sm:min-w-[240px] sm:max-w-sm">
          <Search className="pointer-events-none absolute left-3 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-tx-3" />
          <input
            value={search}
            onChange={(event) => onSearchChange(event.target.value)}
            placeholder={t('extend_tables.search_key_placeholder', {
              key: table.key_field,
            })}
            className="h-9 w-full rounded-md border border-bd-1 bg-bg-2 pl-9 pr-3 text-sm text-tx-0 placeholder:text-tx-3 focus:outline-none focus-visible:bg-bg-3"
          />
        </label>
        <span className="text-xs text-tx-2 sm:ml-auto">
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
        <div className="border-y border-bd-0">
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
              {rows.map((row) => (
                <RecordRows
                  key={row.key}
                  row={row}
                  fields={fields}
                  expanded={expandedKey === row.key}
                  editAccess={editAccess}
                  language={i18n.language}
                  onEdit={onEdit}
                  onDelete={onDelete}
                  onToggleExpanded={() =>
                    onExpandedKeyChange(
                      expandedKey === row.key ? null : row.key,
                    )
                  }
                />
              ))}
            </TableBody>
          </Table>
        </div>
      )}
    </section>
  );
}

function RecordRows({
  row,
  fields,
  expanded,
  editAccess,
  language,
  onEdit,
  onDelete,
  onToggleExpanded,
}: {
  row: ExtendRow;
  fields: ExtendValueField[];
  expanded: boolean;
  editAccess: ActionAccess;
  language: string;
  onEdit: (row: ExtendRow) => void;
  onDelete: (row: ExtendRow) => void;
  onToggleExpanded: () => void;
}) {
  const { t } = useTranslation('functions');
  const values = valueAsObject(row.value_json);
  return (
    <>
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
              className="max-w-[260px] truncate rounded px-1 py-0.5 font-mono text-xs font-strong text-indigo-soft enabled:hover:underline focus-visible:bg-indigo-dim focus-visible:text-indigo disabled:cursor-not-allowed disabled:text-tx-3"
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
          {formatRelativeMicros(row.updated_at_micros, language)}
        </TableCell>
        <TableCell className="text-right">
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <button
                type="button"
                className="inline-grid h-8 w-8 place-items-center rounded-md text-tx-3 hover:bg-bg-3 hover:text-tx-0 focus-visible:bg-bg-3 focus-visible:text-tx-0"
                aria-label={t('extend_tables.row_actions', { key: row.key })}
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
              <DropdownMenuItem onSelect={onToggleExpanded}>
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
            <pre className="max-h-72 overflow-auto whitespace-pre-wrap border-l-2 border-bd-1 bg-bg-0 p-4 font-mono text-xs leading-relaxed text-tx-1">
              {JSON.stringify(row.value_json, null, 2)}
            </pre>
          </TableCell>
        </TableRow>
      )}
    </>
  );
}
