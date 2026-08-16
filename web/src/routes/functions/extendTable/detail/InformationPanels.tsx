import { Code2, KeyRound, Pencil, Trash2, Workflow } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import type {
  ExtendTableSummary,
  ExtendValueField,
} from '@/api/extendTables';
import type { ActionAccess } from '@/product/actionAccess';
import { ChromeButton } from '@/shell/chrome';
import { EmptyState } from '@/shell/EmptyState';
import { cn } from '@/shell/lib/cn';
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/shell/ui/table';

export function SchemaPanel({
  editAccess,
  table,
  fields,
  onEdit,
}: {
  editAccess: ActionAccess;
  table: ExtendTableSummary;
  fields: ExtendValueField[];
  onEdit: () => void;
}) {
  const { t } = useTranslation('functions');
  const rows: Array<ExtendValueField & { keyField?: boolean }> = [
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
    <section data-extend-table-schema>
      <div className="flex flex-wrap items-start justify-between gap-3 px-1 pb-4">
        <div>
          <h2 className="text-sm font-display-strong text-tx-0">
            {t('extend_tables.schema_title')}
          </h2>
          <p className="mt-1 text-xs text-tx-2">
            {t('extend_tables.schema_description')}
          </p>
        </div>
        <ChromeButton
          disabled={editAccess.disabled}
          disabledReason={editAccess.reason}
          onClick={() => editAccess.allowed && onEdit()}
        >
          <Pencil className="h-3.5 w-3.5" />
          {t('extend_tables.edit_schema')}
        </ChromeButton>
      </div>
      <div className="border-y border-bd-0">
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
                    {field.keyField && (
                      <KeyRound className="h-3.5 w-3.5 text-indigo-soft" />
                    )}
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
                  {field.description ||
                    t('extend_tables.no_field_description')}
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      </div>
    </section>
  );
}

export function UsagePanel({ table }: { table: ExtendTableSummary }) {
  const { t } = useTranslation('functions');
  if (table.usage_locations.length === 0) {
    return (
      <EmptyState
        strategy="none"
        icon={Workflow}
        title={t('extend_tables.no_usage_title')}
        description={t('extend_tables.no_usage_description')}
        className="min-h-72"
      />
    );
  }

  return (
    <section data-extend-table-usage>
      <div className="px-1 pb-4">
        <h2 className="text-sm font-display-strong text-tx-0">
          {t('extend_tables.usage_title')}
        </h2>
        <p className="mt-1 text-xs text-tx-2">
          {t('extend_tables.usage_description')}
        </p>
      </div>
      <div className="divide-y divide-bd-0 border-y border-bd-0">
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
              className="flex min-h-16 items-center gap-3 px-1 py-3 hover:bg-bg-2 focus-visible:bg-bg-2 sm:px-4"
            >
              <span className="grid h-9 w-9 shrink-0 place-items-center rounded-md bg-purple-dim text-purple-soft">
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
      </div>
    </section>
  );
}

export function SettingsPanel({
  deleteAccess,
  table,
  onDelete,
}: {
  deleteAccess: ActionAccess;
  table: ExtendTableSummary;
  onDelete: () => void;
}) {
  const { t } = useTranslation('functions');
  return (
    <section
      data-extend-table-settings
      className="max-w-3xl space-y-6"
    >
      <div className="border-y border-bd-0 py-5">
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
      <div className="border-y border-red/25 bg-red-dim px-1 py-5 sm:px-4">
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
  mono?: boolean | undefined;
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
