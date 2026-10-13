import type { TFunction } from 'i18next';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import type * as webApi from '@/api/web';
import { useTimeFormatter } from '@/lib/time';
import { EmptyState } from '@/shell/EmptyState';
import { LONG_LIST_ROW_CLASS, longListRowStyle } from '@/shell/list/longList';
import {
  QueryRecoveryState,
  type QueryRecoveryActions,
  type QueryRecoveryCopy,
} from '@/shell/query/RecoveryState';
import type { queryStateFor } from '@/shell/query/State';
import { formatTraceDurationMs } from '@/viz/trace/duration';
import { TraceFlame } from '@/viz/trace/TraceFlame';
import { TraceOperationName } from '@/viz/trace/TraceOperationName';

import {
  TracePagination,
  type TracePaginationModel,
} from '../Pagination';
import type { TraceHistogramModel } from './Histogram';
import { TraceResultsHead } from './ResultsHead';
import { formatTraceStart } from './shared';
import type { DisplayTrace } from './types';

interface TraceSpanExplorerProps {
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
  selectedId: string | null;
  onSelectTrace: (id: string) => void;
  pagination: TracePaginationModel;
  histogram: TraceHistogramModel;
  onSortChange: (sort: webApi.TraceListSort) => void;
}

export function TraceSpanExplorer({
  fieldPanel,
  traceList,
  loadedTraceCount,
  sort,
  sortEnabled,
  listState,
  listError,
  recovery,
  recoveryCopy,
  selectedId,
  onSelectTrace,
  pagination,
  histogram,
  onSortChange,
}: TraceSpanExplorerProps) {
  const { t } = useTranslation('traces');
  const fmt = useTimeFormatter();

  return (
    <div
      data-trace-workspace-layout="surface-grid"
      className="flex h-full min-h-0 w-full items-stretch gap-[8px]"
    >
      {fieldPanel}
      {/* The results under one head, as in the Traces view: the histogram spans the list and the
          open trace alike, since it is of all the traces and not of the list's column. */}
      <div className="flex min-h-0 min-w-0 flex-1 flex-col gap-[8px]">
        <TraceResultsHead
          loadedTraceCount={loadedTraceCount}
          sort={sort}
          sortEnabled={sortEnabled}
          onSortChange={onSortChange}
          histogram={histogram}
          className="overflow-hidden rounded-md bg-[var(--functional-surface)]"
        />
        <div className="flex min-h-0 flex-1 items-stretch gap-[8px]">
          <section
            data-workspace-pane="trace-results"
            className="flex min-h-0 w-[380px] shrink-0 flex-col overflow-hidden rounded-md bg-[var(--functional-surface)] [contain:size]"
          >
            {/* A trace is a row from edge to edge with a line under it, as in the Traces view. */}
            <div className="min-h-0 flex-1 overflow-y-auto">
              {listState ? (
                <QueryRecoveryState
                  state={listState}
                  error={listError}
                  copy={recoveryCopy}
                  recovery={recovery}
                  className="h-full min-h-0"
                  testId="traces-no-data"
                />
              ) : (
                traceList.map((trace) => (
                  <TraceRow
                    key={trace.id}
                    trace={trace}
                    selected={selectedId === trace.id}
                    tz={fmt.tz}
                    t={t}
                    onSelect={onSelectTrace}
                  />
                ))
              )}
            </div>
            <TracePagination model={pagination} />
          </section>

          <section
            data-workspace-pane="trace-detail"
            className="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden rounded-md bg-[var(--functional-surface)] [contain:size]"
          >
            <div className="flex h-11 shrink-0 items-center gap-2 px-3 font-sans text-xs text-tx-2">
              <span className="min-w-0 flex-1 truncate">
                {selectedId ? selectedId : t('detail.title')}
              </span>
              {selectedId && (
                <Link
                  to={`/traces/${encodeURIComponent(selectedId)}`}
                  className="shrink-0 whitespace-nowrap text-[var(--blue-ink)] hover:underline"
                >
                  {t('explore.results.open_detail')}
                </Link>
              )}
            </div>
            <div className="min-h-0 flex-1 overflow-auto p-4">
              {selectedId ? (
                <TraceFlame traceId={selectedId} />
              ) : (
                <EmptyState
                  strategy="query-first"
                  title={t('explore.results.select_prompt')}
                  description={t('explore.results.select_description')}
                  className="min-h-0"
                />
              )}
            </div>
          </section>
        </div>
      </div>
    </div>
  );
}

/** Two lines of sixteen and the two between them; the row adds its own padding and the line under it. */
const TRACE_ROW_STYLE = longListRowStyle(34);

/**
 * One trace of the list. It is drawn again only when it changes itself:
 * opening a trace redraws the row that was open and the one that is, not the
 * thousands a page can hold.
 */
const TraceRow = React.memo(function TraceRow({
  trace,
  selected,
  tz,
  t,
  onSelect,
}: {
  trace: DisplayTrace;
  selected: boolean;
  tz: string;
  t: TFunction<'traces'>;
  onSelect: (id: string) => void;
}) {
  return (
    <button
      type="button"
      title={trace.id}
      aria-label={t('explore.results.select_trace_aria', { operation: trace.op })}
      aria-pressed={selected}
      onClick={() => onSelect(trace.id)}
      style={TRACE_ROW_STYLE}
      className={`block w-full border-b border-bd-0 px-[16px] py-[8px] text-left font-sans text-xs leading-[16px] transition-colors ${LONG_LIST_ROW_CLASS} ${
        // The trace that is open keeps its fill under the pointer, and deepens it under the
        // keyboard. On the deeper fill the quiet and the coloured text would be too faint
        // to read, so there every word of the row is set in the main text colour.
        selected
          ? 'bg-indigo-dim focus-visible:bg-indigo/25 [&:focus-visible_*]:text-tx-0'
          : 'bg-transparent hover:bg-bg-2 focus-visible:bg-bg-2'
      }`}
    >
      {/* Both lines are a fixed height: a name set as an HTTP method is a fraction
          taller on its baseline, and the rows would stop being one height. */}
      <span className="grid h-[16px] grid-cols-[minmax(0,1fr)_auto] items-baseline gap-3">
        <TraceOperationName operation={trace.op} className="font-semibold text-tx-0" />
        <span className="whitespace-nowrap font-mono font-bold tabular-nums text-tx-0">
          {formatTraceDurationMs(trace.durationMs)}
        </span>
      </span>
      <span className="mt-[2px] grid h-[16px] grid-cols-[minmax(0,1fr)_auto] items-center gap-3">
        <span className="flex min-w-0 items-center gap-1.5 overflow-hidden text-tx-2">
          <span className="truncate">{trace.service}</span>
          <span className="shrink-0 text-tx-4">·</span>
          <span className="shrink-0">{t('explore.results.span_count', { count: trace.spans })}</span>
          {trace.errors > 0 ? (
            <>
              <span className="shrink-0 text-tx-4">·</span>
              <span className="shrink-0 font-semibold text-red-soft">
                {t('explore.results.error_count', { count: trace.errors })}
              </span>
            </>
          ) : null}
        </span>
        <time className="whitespace-nowrap font-mono text-tx-3">{formatTraceStart(trace.startNs, tz)}</time>
      </span>
    </button>
  );
});
