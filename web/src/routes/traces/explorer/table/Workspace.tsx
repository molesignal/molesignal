import { GitCompare } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';

import type * as webApi from '@/api/web';
import { ModuleToggle } from '@/shell/query/ModuleToggle';
import {
  QueryRecoveryState,
  type QueryRecoveryActions,
  type QueryRecoveryCopy,
} from '@/shell/query/RecoveryState';
import type { queryStateFor } from '@/shell/query/State';
import { traceCompareHref } from '@/viz/trace/compare/link';
import { tracePageHref } from '@/viz/trace/pageLink';

import { TracePagination, type TracePaginationModel } from '../../Pagination';
import { backTrace, followedTrace, listedTraceWindow, openedTrace, type TraceDrawerState } from '../drawer/model';
import { TraceDetailDrawer } from '../drawer/TraceDrawer';
import type { TraceHistogramModel } from '../Histogram';
import { TraceResultsHead } from '../ResultsHead';
import type { DisplayTrace } from '../types';
import { TraceColumnMenu } from './ColumnMenu';
import { ComparePickBar } from './ComparePick';
import { TraceTable } from './TraceTable';
import type { useTraceColumns } from './useTraceColumns';

interface TraceTableExplorerProps {
  /** The field list, beside the results. */
  fieldPanel: React.ReactNode;
  traceList: DisplayTrace[];
  loadedTraceCount: number;
  sort: webApi.TraceListSort;
  sortEnabled: boolean;
  listState: ReturnType<typeof queryStateFor>;
  listError: unknown;
  recovery: QueryRecoveryActions;
  recoveryCopy: QueryRecoveryCopy;
  /** The columns of the table as the reader arranged them, and the ways to arrange them. */
  columns: ReturnType<typeof useTraceColumns>;
  pagination: TracePaginationModel;
  histogram: TraceHistogramModel;
  onSortChange: (sort: webApi.TraceListSort) => void;
}

/**
 * The Traces view: the field list, the head with its histogram and latency,
 * the table of traces, and a trace opened from it in a drawer over it. The
 * list behind stays as it was left.
 */
export function TraceTableExplorer({
  fieldPanel,
  traceList,
  loadedTraceCount,
  sort,
  sortEnabled,
  listState,
  listError,
  recovery,
  recoveryCopy,
  columns,
  pagination,
  histogram,
  onSortChange,
}: TraceTableExplorerProps) {
  const { t } = useTranslation('traces');
  const navigate = useNavigate();
  const [drawer, setDrawer] = React.useState<TraceDrawerState | null>(null);
  // The row that opened the drawer: closing the drawer focuses it again.
  const opener = React.useRef<HTMLElement | null>(null);
  const listed = drawer?.stack[0]?.traceId ?? null;
  const openId = drawer?.open ? listed : null;
  // The traces picked to compare, while they are being picked: the latest two clicked.
  const [picked, setPicked] = React.useState<DisplayTrace[] | null>(null);
  const pickedIds = React.useMemo(() => picked?.map((trace) => trace.id) ?? null, [picked]);

  const pick = React.useCallback((trace: DisplayTrace) => {
    setPicked((current) => {
      if (!current) return current;
      if (current.some((item) => item.id === trace.id)) return current.filter((item) => item.id !== trace.id);
      return [...current, trace].slice(-2);
    });
  }, []);

  const startPicking = (on: boolean) => {
    setPicked(on ? [] : null);
    if (on) setDrawer((current) => current && { ...current, open: false });
  };

  const comparePicked = () => {
    const [first, second] = picked ?? [];
    if (!first || !second) return;
    navigate(
      traceCompareHref(
        { traceId: first.id, window: listedTraceWindow(first.startNs, first.durationMs) },
        { traceId: second.id, window: listedTraceWindow(second.startNs, second.durationMs) },
      ),
    );
  };

  const open = React.useCallback((trace: DisplayTrace, row?: HTMLElement) => {
    if (row) opener.current = row;
    setDrawer(openedTrace({ traceId: trace.id, window: listedTraceWindow(trace.startNs, trace.durationMs) }));
  }, []);

  const index = traceList.findIndex((trace) => trace.id === listed);
  const step = (by: 1 | -1) => {
    const next = traceList[index + by];
    if (!next) return;
    opener.current = document.querySelector<HTMLElement>(`[data-trace-row][data-trace-id="${CSS.escape(next.id)}"]`) ?? opener.current;
    open(next);
  };

  return (
    // The field list and the results are one surface, the list set off by a line, not two cards apart.
    <div
      data-trace-workspace-layout="joined"
      className="flex h-full min-h-0 w-full items-stretch overflow-hidden rounded-md bg-[var(--functional-surface)]"
    >
      {fieldPanel}
      <div data-workspace-pane="trace-results" className="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden [contain:size]">
        <TraceResultsHead
          loadedTraceCount={loadedTraceCount}
          histogram={histogram}
          actions={
            <>
              <ModuleToggle
                size="sm"
                open={picked !== null}
                icon={GitCompare}
                actionLabel={t(picked ? 'explore.compare.stop' : 'explore.compare.start')}
                onOpenChange={startPicking}
              />
              <TraceColumnMenu
              columns={columns.columns}
              available={columns.available}
              full={columns.full}
              arranged={columns.arranged}
              onToggle={columns.toggle}
              onReorder={columns.reorder}
              onReset={columns.reset}
              />
            </>
          }
          latency
          bandClassName="mb-[4px]"
        />
        {picked && (
          <ComparePickBar
            picked={picked}
            onUnpick={pick}
            onCompare={comparePicked}
            onCancel={() => setPicked(null)}
          />
        )}
        <TraceTable
          traces={listState ? [] : traceList}
          columns={columns.columns}
          sort={sort}
          sortEnabled={sortEnabled}
          onSortChange={onSortChange}
          openId={openId}
          onOpen={picked ? pick : open}
          picked={pickedIds}
          empty={
            listState ? (
              <QueryRecoveryState
                state={listState}
                error={listError}
                copy={recoveryCopy}
                recovery={recovery}
                className="h-full min-h-0"
                testId="traces-table-no-data"
              />
            ) : null
          }
        />
        <TracePagination model={pagination} />
      </div>
      <TraceDetailDrawer
        state={drawer}
        onClose={() => setDrawer((current) => current && { ...current, open: false })}
        onFollow={(trace) => setDrawer((current) => (current ? followedTrace(current, trace) : openedTrace(trace)))}
        onBack={() => setDrawer((current) => current && backTrace(current))}
        neighbours={{
          ...(index > 0 ? { onPrevious: () => step(-1) } : {}),
          ...(index >= 0 && index < traceList.length - 1 ? { onNext: () => step(1) } : {}),
        }}
        pageHref={tracePageHref}
        returnFocusRef={opener}
      />
    </div>
  );
}

