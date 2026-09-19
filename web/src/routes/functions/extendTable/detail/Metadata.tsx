import {
  Braces,
  Clock3,
  KeyRound,
  type LucideIcon,
  Rows3,
  Settings2,
  Workflow,
} from 'lucide-react';
import { useTranslation } from 'react-i18next';

import type { ExtendTableSummary } from '@/api/extendTables';
import { cn } from '@/shell/lib/cn';
import { Tabs, TabsList, TabsTrigger } from '@/shell/ui/tabs';

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
    <div className="overflow-hidden rounded-md bg-[var(--functional-surface)] px-3 [box-shadow:var(--shadow-functional-surface)]">
      <Tabs value={value} onValueChange={(next) => onChange(next as DetailTab)}>
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
            count={fieldCount + 1}
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
  count?: number | undefined;
}) {
  return (
    <TabsTrigger
      value={value}
      className="h-10 gap-2 rounded-none border-b-[3px] border-transparent bg-transparent px-4 text-xs text-tx-2 shadow-none data-[state=active]:border-indigo data-[state=active]:bg-transparent data-[state=active]:text-tx-0 data-[state=active]:shadow-none"
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

export function DetailSkeleton() {
  return (
    <div className="animate-pulse space-y-5">
      <div className="h-20 bg-bg-2" />
      <div className="h-10 w-96 max-w-full bg-bg-2" />
      <div className="h-80 bg-bg-2" />
    </div>
  );
}
