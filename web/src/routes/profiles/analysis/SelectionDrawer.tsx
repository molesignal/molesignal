import { useTranslation } from 'react-i18next';

import { DataTable, type DataTableColumn } from '@/admin';
import type { ProfileEntry } from '@/api/profiles';
import { formatMicrosActive } from '@/lib/time';
import { SignalReference } from '@/shell/SignalReference';
import { Checkbox } from '@/shell/ui/checkbox';
import { Sheet, SheetContent, SheetDescription, SheetTitle } from '@/shell/ui/sheet';
import {
  formatCount,
  formatDuration,
  formatValue,
  unitForProfileType,
} from '@/viz/profiles/flamebearer';

import { useProfileTypeLabel } from '../shared';

/**
 * The profiles the flame graph is merged from, in a panel beside it: which
 * ones take part is the scope of the analysis, and it is changed from where
 * the scope is said, not from the foot of the page. Ticking a profile in or
 * out merges the graph again; a row opens that profile on its own.
 */
export function ProfileSelectionDrawer({
  open,
  onOpenChange,
  rows,
  excluded,
  onToggle,
  onToggleAll,
  onOpenProfile,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  rows: ProfileEntry[];
  /** The profiles taken out of the analysis. */
  excluded: ReadonlySet<string>;
  onToggle: (id: string, selected: boolean) => void;
  onToggleAll: (selected: boolean) => void;
  onOpenProfile: (row: ProfileEntry) => void;
}) {
  const { t } = useTranslation('profiles');
  const typeLabel = useProfileTypeLabel();
  const selectedCount = rows.filter((row) => !excluded.has(row.id)).length;
  const allSelected = rows.length > 0 && selectedCount === rows.length;
  const someSelected = selectedCount > 0 && !allSelected;

  const columns: DataTableColumn<ProfileEntry>[] = [
    {
      key: 'selected',
      header: (
        <div className="flex justify-center" onClick={(event) => event.stopPropagation()}>
          <Checkbox
            checked={allSelected ? true : someSelected ? 'indeterminate' : false}
            onCheckedChange={(checked) => onToggleAll(checked === true)}
            aria-label={allSelected ? t('selection.select_none') : t('selection.select_all')}
          />
        </div>
      ),
      cell: (row) => (
        <div
          className="flex justify-center"
          onClick={(event) => event.stopPropagation()}
          onKeyDown={(event) => event.stopPropagation()}
        >
          <Checkbox
            checked={!excluded.has(row.id)}
            onCheckedChange={(checked) => onToggle(row.id, checked === true)}
            aria-label={t('selection.row_aria', { id: row.id })}
          />
        </div>
      ),
      width: 44,
      className: 'px-2',
      headerClassName: 'px-2',
    },
    {
      key: 'service',
      header: t('list.columns.service'),
      cell: (row) => <span className="text-tx-0">{row.service}</span>,
    },
    {
      key: 'type',
      header: t('list.columns.type'),
      cell: (row) => typeLabel(row.profile_type),
      width: 130,
    },
    {
      key: 'timestamp',
      header: t('list.columns.timestamp'),
      cell: (row) => formatMicrosActive(row.timestamp),
      width: 170,
    },
    {
      key: 'duration',
      header: <span title={t('list.tooltip.duration')}>{t('list.columns.duration')}</span>,
      cell: (row) => formatDuration(row.duration_nanos),
      width: 90,
    },
    {
      key: 'samples',
      header: t('list.columns.samples'),
      cell: (row) => formatCount(row.sample_count),
      width: 80,
    },
    {
      key: 'total',
      header: <span title={t('list.tooltip.total')}>{t('list.columns.total_value')}</span>,
      cell: (row) => formatValue(row.total_value, unitForProfileType(row.profile_type)),
      width: 100,
    },
  ];

  if (rows.some((row) => row.trace_id)) {
    columns.push({
      key: 'trace',
      header: t('list.columns.trace'),
      cell: (row) =>
        row.trace_id ? (
          <SignalReference type="trace_id" value={row.trace_id}>
            {row.trace_id.slice(0, 12)}
          </SignalReference>
        ) : (
          <span className="text-tx-3">—</span>
        ),
      width: 130,
    });
  }

  return (
    <Sheet open={open} onOpenChange={onOpenChange}>
      <SheetContent
        side="right"
        data-profile-selection
        className="flex w-full flex-col gap-0 border-0 bg-[var(--functional-surface)] p-0 sm:max-w-[820px]"
      >
        <div className="shrink-0 px-[16px] pb-[10px] pt-[14px] pr-[44px]">
          <SheetTitle className="flex flex-wrap items-baseline gap-x-[10px] font-sans text-sm font-strong text-tx-0">
            {t('selection.title')}
            <span data-profile-selection-count className="font-sans text-xs font-body tabular-nums text-tx-2">
              {t('selection.selected_summary', { selected: selectedCount, total: rows.length })}
            </span>
          </SheetTitle>
          <SheetDescription className="mt-[4px] font-sans text-xs text-tx-2">
            {t('selection.description')}
          </SheetDescription>
        </div>
        <div className="min-h-0 flex-1 overflow-auto">
          <DataTable
            className="rounded-none border-0"
            rows={rows}
            rowKey={(row) => row.id}
            onRowClick={onOpenProfile}
            columns={columns}
          />
        </div>
      </SheetContent>
    </Sheet>
  );
}
