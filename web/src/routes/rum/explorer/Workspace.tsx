import { Download, RotateCcw } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { RumField, RumRow } from '@/api/rum';
import { CursorPagination } from '@/shell/CursorPagination';
import { cn } from '@/shell/lib/cn';
import { QueryIconAction } from '@/shell/query/IconAction';
import {
  QueryRecoveryState,
  type QueryRecoveryActions,
  type QueryRecoveryCopy,
} from '@/shell/query/RecoveryState';
import type { queryStateFor } from '@/shell/query/State';
import { QuerySplitToggle } from '@/shell/queryTabs';

import { type DrawerNeighbours, RumDetailDrawer, type RumDrawerState } from './Drawer';
import { RumHistogramBand, type RumHistogramModel, RumHistogramToggle } from './Histogram';
import { type RumTab, rowId } from './model';
import { RumTable } from './Table';
import type { RumPaginationModel } from './useRumList';

interface RumWorkspaceProps {
  tab: RumTab;
  /** The field list, beside the results. */
  fieldPanel: React.ReactNode;
  /** Every field of the type: the drawer formats what it shows by them. */
  fields: readonly RumField[];
  /** The columns of the table as the reader arranged them. */
  columns: readonly RumField[];
  rows: readonly RumRow[];
  listState: ReturnType<typeof queryStateFor>;
  listError: unknown;
  recovery: QueryRecoveryActions;
  recoveryCopy: QueryRecoveryCopy;
  pagination: RumPaginationModel;
  histogram: RumHistogramModel;
  /** The columns are not the ones the type opens with. */
  arranged: boolean;
  onResetColumns: () => void;
  /** The switch between the rows and the ways of drawing numbers over them. */
  vizSwitch: React.ReactNode;
  /** What is drawn in the rows' place when they are drawn as numbers; none while the list is in front. */
  vizBody: React.ReactNode | null;
  /** What is said of what is drawn: its range, its points and its groups. */
  vizStatus: string;
  /** Writes the rows the query matches to a file. */
  onExport: () => Promise<void>;
}

/**
 * The explorer under its bar: the field list, the head with its histogram, the table of rows, and
 * a row opened from it in a drawer over it. The list behind stays as it was left. The head also
 * says how the rows are drawn: as numbers over them, they take the table's place.
 */
export function RumWorkspace({
  tab,
  fieldPanel,
  fields,
  columns,
  rows,
  listState,
  listError,
  recovery,
  recoveryCopy,
  pagination,
  histogram,
  arranged,
  onResetColumns,
  vizSwitch,
  vizBody,
  vizStatus,
  onExport,
}: RumWorkspaceProps) {
  const { t } = useTranslation('rum');
  const [exporting, setExporting] = React.useState(false);
  const [drawer, setDrawer] = React.useState<RumDrawerState | null>(null);
  // The row that opened the drawer: closing the drawer focuses it again.
  const opener = React.useRef<HTMLElement | null>(null);
  const openId = drawer?.open ? rowId(drawer.tab, drawer.row) : null;

  // Another type is another list: a row of the one before is not open in it.
  React.useEffect(() => setDrawer(null), [tab]);

  const open = React.useCallback(
    (row: RumRow, element?: HTMLElement) => {
      if (element) opener.current = element;
      setDrawer({ tab, row, open: true });
    },
    [tab],
  );

  const index = drawer ? rows.findIndex((row) => rowId(tab, row) === rowId(tab, drawer.row)) : -1;

  const step = (by: 1 | -1) => {
    const next = rows[index + by];
    if (!next) return;
    opener.current =
      document.querySelector<HTMLElement>(`[data-rum-row][data-rum-id="${CSS.escape(rowId(tab, next))}"]`) ??
      opener.current;
    open(next);
  };

  const neighbours: DrawerNeighbours = {
    ...(index > 0 ? { onPrevious: () => step(-1) } : {}),
    ...(index >= 0 && index < rows.length - 1 ? { onNext: () => step(1) } : {}),
  };

  return (
    // The field list and the results are one surface, the list set off by a line, not two cards apart.
    <div
      data-rum-workspace="joined"
      className="flex h-full min-h-0 w-full items-stretch overflow-hidden rounded-md bg-[var(--functional-surface)]"
    >
      {fieldPanel}
      <div data-workspace-pane="rum-results" className="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden [contain:size]">
        <div data-rum-results-head className="shrink-0">
          <div className="flex min-h-11 items-center justify-between gap-3 px-3 py-1.5">
            {vizSwitch}
            {/* What is said of the results, then their signs, the switch that splits the page first. */}
            <div className={cn('flex min-w-0 items-center gap-3', !vizBody && 'shrink-0')}>
              {vizBody ? (
                <span data-rum-viz-status="" className="min-w-0 truncate font-sans text-xs text-tx-3">
                  {vizStatus}
                </span>
              ) : (
                <span className="min-w-0 truncate font-sans text-xs font-semibold text-tx-0">
                  {t('explorer.results.loaded_count', { count: rows.length })}
                </span>
              )}
              <div className="flex shrink-0 items-center gap-1.5 empty:hidden">
                <QuerySplitToggle size="sm" />
                {!vizBody && arranged && (
                  <QueryIconAction
                    icon={RotateCcw}
                    label={t('explorer.results.reset_columns')}
                    onClick={onResetColumns}
                  />
                )}
                {!vizBody && <RumHistogramToggle model={histogram} />}
                {!vizBody && (
                  <QueryIconAction
                    icon={Download}
                    label={t('explorer.results.export')}
                    data-rum-export=""
                    disabled={exporting || rows.length === 0}
                    onClick={() => {
                      setExporting(true);
                      void onExport().finally(() => setExporting(false));
                    }}
                  />
                )}
              </div>
            </div>
          </div>
          {!vizBody && <RumHistogramBand model={histogram} tab={tab} className="mb-[4px]" />}
        </div>
        {vizBody ?? (
          <>
            <RumTable
              tab={tab}
              columns={columns}
              rows={listState ? [] : rows}
              openId={openId}
              onOpen={open}
              empty={
                listState ? (
                  <QueryRecoveryState
                    state={listState}
                    error={listError}
                    copy={recoveryCopy}
                    recovery={recovery}
                    className="h-full min-h-0"
                    testId="rum-table-no-data"
                  />
                ) : null
              }
            />
            <CursorPagination
              pageSize={pagination.pageSize}
              pageSizeOptions={pagination.pageSizeOptions}
              hasPrevious={pagination.hasPrevious}
              hasNext={pagination.hasNext}
              pending={pagination.pending}
              ariaLabel={t('explorer.pagination.aria')}
              pageSizeAriaLabel={t('explorer.pagination.page_size_aria')}
              previousLabel={t('explorer.pagination.previous')}
              nextLabel={t('explorer.pagination.next')}
              onPrevious={pagination.onPrevious}
              onNext={pagination.onNext}
              onPageSizeChange={pagination.onPageSizeChange}
              className="border-t-0 bg-transparent"
            />
          </>
        )}
      </div>
      <RumDetailDrawer
        state={drawer}
        fields={fields}
        onClose={() => setDrawer((current) => current && { ...current, open: false })}
        neighbours={neighbours}
        returnFocusRef={opener}
      />
    </div>
  );
}
