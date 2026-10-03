import {
  Braces,
  Clock3,
  KeyRound,
  Rows3,
  Settings2,
  Workflow,
} from 'lucide-react';
import { useTranslation } from 'react-i18next';

import type { ExtendTableSummary } from '@/api/extendTables';
import { cn } from '@/shell/lib/cn';
import { ModuleTabs } from '@/shell/tabs/ModuleTabs';

import type { DetailTab } from './types';
import { formatRelativeMicros } from '../../../pipelines/presentation';

export function TableMetadata({ table }: { table: ExtendTableSummary }) {
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
    <div
      data-extend-table-metadata
      className="grid gap-[12px] sm:grid-cols-2 xl:grid-cols-4"
    >
      {items.map((item) => (
        <div
          key={item.label}
          className="flex min-h-16 items-center gap-3 rounded-md bg-[var(--functional-surface)] px-4 py-2 [box-shadow:var(--shadow-functional-surface)]"
        >
          <span className="grid h-9 w-9 shrink-0 place-items-center rounded-md bg-bg-2 text-tx-2">
            <item.icon className="h-4 w-4" />
          </span>
          <span className="min-w-0">
            <span className="block text-type-micro tracking-normalr text-tx-3">
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

export function DetailTabs({
  value,
  table,
  fieldCount,
  onChange,
}: {
  value: DetailTab;
  table: ExtendTableSummary;
  fieldCount: number;
  onChange: (value: DetailTab) => void;
}) {
  const { t } = useTranslation('functions');
  return (
    <ModuleTabs
      label={t('extend_tables.tabs.label')}
      className="mx-0 mb-0"
      value={value}
      onValueChange={(next) => onChange(next as DetailTab)}
      items={[
        {
          key: 'records',
          icon: Rows3,
          label: t('extend_tables.tabs.records'),
          count: table.row_count,
        },
        {
          key: 'schema',
          icon: Braces,
          label: t('extend_tables.tabs.schema'),
          count: fieldCount + 1,
        },
        {
          key: 'usage',
          icon: Workflow,
          label: t('extend_tables.tabs.usage'),
          count: table.usage_locations.length,
        },
        { key: 'settings', icon: Settings2, label: t('extend_tables.tabs.settings') },
      ]}
    />
  );
}

export function DetailSkeleton() {
  return (
    <div className="animate-pulse space-y-5">
      <div className="h-20 bg-bg-2" />
      <div className="h-10 w-96 max-w-full bg-bg-2" />
      <div className="h-80 bg-bg-2" />
    </div>
  );
}
