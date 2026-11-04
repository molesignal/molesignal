import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { SpanListItem, SpanListSort } from '@/api/traceExplorer';
import {
  QueryRecoveryState,
  type QueryRecoveryActions,
  type QueryRecoveryCopy,
} from '@/shell/query/RecoveryState';
import type { queryStateFor } from '@/shell/query/State';
import { tracePageHref } from '@/viz/trace/pageLink';

import { TracePagination, type TracePaginationModel } from '../../Pagination';
import { backTrace, type DrawerTrace, followedTrace, openedTrace, type TraceDrawerState } from '../drawer/model';
import { TraceDetailDrawer } from '../drawer/TraceDrawer';
import type { TraceHistogramModel } from '../Histogram';
import { TraceResultsHead } from '../ResultsHead';
import { spanTraceWindow } from './model';
import { spanKey, SpanTable } from './SpanTable';

interface SpanListExplorerProps {
  fieldPanel: React.ReactNode;
  spans: SpanListItem[];
  sort: SpanListSort;
  sortEnabled: boolean;
  listState: ReturnType<typeof queryStateFor>;
  listError: unknown;
  recovery: QueryRecoveryActions;
  recoveryCopy: QueryRecoveryCopy;
  pagination: TracePaginationModel;
  histogram: TraceHistogramModel;
  onSortChange: (sort: SpanListSort) => void;
}

/**
 * The Spans view: one row for each span the query matches, under the same
 * head as the traces, the histogram counting spans. A span opens its trace
 * in the drawer, at that span.
 */
export function SpanListExplorer({
  fieldPanel,
  spans,
  sort,
  sortEnabled,
  listState,
  listError,
  recovery,
  recoveryCopy,
  pagination,
  histogram,
  onSortChange,
}: SpanListExplorerProps) {
  const { t } = useTranslation('traces');
  const [drawer, setDrawer] = React.useState<TraceDrawerState | null>(null);
  const [openKey, setOpenKey] = React.useState<string | null>(null);
  const opener = React.useRef<HTMLElement | null>(null);

  const open = React.useCallback((span: SpanListItem, row?: HTMLElement) => {
    if (row) opener.current = row;
    setOpenKey(spanKey(span));
    setDrawer(openedTrace({ traceId: span.traceId, spanId: span.spanId, window: spanTraceWindow(span.startNs) }));
  }, []);

  const index = spans.findIndex((span) => spanKey(span) === openKey);
  const step = (by: 1 | -1) => {
    const next = spans[index + by];
    if (next) open(next);
  };

  return (
    // The field list and the results are one surface, the list set off by a line, not two cards apart.
    <div
      data-trace-workspace-layout="joined"
      className="flex h-full min-h-0 w-full items-stretch overflow-hidden rounded-md bg-[var(--functional-surface)]"
    >
      {fieldPanel}
      <div data-workspace-pane="span-results" className="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden [contain:size]">
        <TraceResultsHead
          loadedTraceCount={spans.length}
          summary={t('explore.spans.loaded_count', { count: spans.length })}
          histogram={histogram}
          unit="spans"
          latency
          bandClassName="mb-[4px]"
        />
        <SpanTable
          spans={listState ? [] : spans}
          sort={sort}
          sortEnabled={sortEnabled}
          onSortChange={onSortChange}
          openKey={drawer?.open ? openKey : null}
          onOpen={open}
          empty={
            listState ? (
              <QueryRecoveryState
                state={listState}
                error={listError}
                copy={recoveryCopy}
                recovery={recovery}
                className="h-full min-h-0"
                testId="spans-no-data"
              />
            ) : null
          }
        />
        <TracePagination model={pagination} />
      </div>
      <TraceDetailDrawer
        state={drawer}
        onClose={() => setDrawer((current) => current && { ...current, open: false })}
        onFollow={(trace: DrawerTrace) => setDrawer((current) => (current ? followedTrace(current, trace) : openedTrace(trace)))}
        onBack={() => setDrawer((current) => current && backTrace(current))}
        neighbours={{
          ...(index > 0 ? { onPrevious: () => step(-1) } : {}),
          ...(index >= 0 && index < spans.length - 1 ? { onNext: () => step(1) } : {}),
        }}
        pageHref={tracePageHref}
        returnFocusRef={opener}
      />
    </div>
  );
}
