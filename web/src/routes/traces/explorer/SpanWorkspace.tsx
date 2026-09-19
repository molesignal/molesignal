import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import type * as webApi from '@/api/web';
import { useTimeFormatter } from '@/lib/time';
import { EmptyState } from '@/shell/EmptyState';
import {
  QueryRecoveryState,
  type QueryRecoveryActions,
  type QueryRecoveryCopy,
} from '@/shell/query/RecoveryState';
import type { queryStateFor } from '@/shell/query/State';
import { formatTraceDurationMs } from '@/viz/trace/duration';
import { TraceFlame } from '@/viz/trace/TraceFlame';
import { TraceOperationName } from '@/viz/trace/TraceOperationName';

import { TraceFieldPanel } from '../fieldPanel/Panel';
import {
  type TraceFieldDef,
  type TraceFieldName,
  type TraceQueryMode,
} from '../fieldQueryModel';
import {
  TracePagination,
  type TracePaginationModel,
} from '../Pagination';
import { formatTraceStart, TraceListSortSelect } from './shared';
import type { DisplayTrace } from './types';

interface TraceSpanExplorerProps {
  traceList: DisplayTrace[];
  fieldTraces: DisplayTrace[];
  loadedTraceCount: number;
  sort: webApi.TraceListSort;
  sortEnabled: boolean;
  queryMode: TraceQueryMode;
  fields: TraceFieldDef[];
  listState: ReturnType<typeof queryStateFor>;
  listError: unknown;
  recovery: QueryRecoveryActions;
  recoveryCopy: QueryRecoveryCopy;
  selectedId: string | null;
  visibleFields: TraceFieldName[];
  fieldFilter: string;
  fieldPanelCollapsed: boolean;
  onFieldFilterChange: (value: string) => void;
  onFieldPanelCollapsedChange: (collapsed: boolean) => void;
  onToggleField: (field: TraceFieldName) => void;
  onInsertField: (field: TraceFieldDef) => void;
  onSelectTrace: (id: string) => void;
  pagination: TracePaginationModel;
  onSortChange: (sort: webApi.TraceListSort) => void;
}

export function TraceSpanExplorer({
  traceList,
  fieldTraces,
  loadedTraceCount,
  sort,
  sortEnabled,
  queryMode,
  fields,
  listState,
  listError,
  recovery,
  recoveryCopy,
  selectedId,
  visibleFields,
  fieldFilter,
  fieldPanelCollapsed,
  onFieldFilterChange,
  onFieldPanelCollapsedChange,
  onToggleField,
  onInsertField,
  onSelectTrace,
  pagination,
  onSortChange,
}: TraceSpanExplorerProps) {
  const { t } = useTranslation('traces');
  const fmt = useTimeFormatter();

  return (
    <div
      data-trace-workspace-layout="surface-grid"
      className="flex h-full min-h-0 w-full items-stretch gap-[8px]"
    >
      <TraceFieldPanel
        traces={fieldTraces}
        fields={fields}
        queryMode={queryMode}
        visibleFields={visibleFields}
        fieldFilter={fieldFilter}
        collapsed={fieldPanelCollapsed}
        onFieldFilterChange={onFieldFilterChange}
        onCollapsedChange={onFieldPanelCollapsedChange}
        onToggleField={onToggleField}
        onInsertField={onInsertField}
        className="rounded-md border-r-0 bg-[var(--functional-surface)]"
      />
      <section
        data-workspace-pane="trace-results"
        className="flex min-h-0 w-[380px] shrink-0 flex-col overflow-hidden rounded-md bg-[var(--functional-surface)] [contain:size]"
      >
        <div className="flex h-11 shrink-0 items-center justify-between gap-3 px-3 font-sans text-xs">
          <span className="min-w-0 truncate text-tx-1">
            {t('explore.results.loaded_count', { count: loadedTraceCount })}
          </span>
          {sortEnabled ? (
            <TraceListSortSelect value={sort} onChange={onSortChange} />
          ) : null}
        </div>
        <div className="min-h-0 flex-1 overflow-y-auto px-2">
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
            traceList.map((trace) => {
              const selected = selectedId === trace.id;
              return (
                <button
                  key={trace.id}
                  type="button"
                  title={trace.id}
                  aria-label={t('explore.results.select_trace_aria', { operation: trace.op })}
                  aria-pressed={selected}
                  onClick={() => onSelectTrace(trace.id)}
                  className={`mb-1 block min-h-16 w-full rounded-md px-3 py-2.5 text-left font-sans text-xs transition-colors hover:bg-bg-2 focus-visible:bg-bg-2 ${
                    selected ? 'bg-indigo-dim' : 'bg-transparent'
                  }`}
                >
                  <span className="grid grid-cols-[minmax(0,1fr)_auto] items-baseline gap-3">
                    <TraceOperationName operation={trace.op} className="font-semibold text-tx-0" />
                    <span className="whitespace-nowrap font-mono font-bold tabular-nums text-tx-0">
                      {formatTraceDurationMs(trace.durationMs)}
                    </span>
                  </span>
                  <span className="mt-1.5 grid grid-cols-[minmax(0,1fr)_auto] items-center gap-3">
                    <span className="flex min-w-0 items-center gap-1.5 overflow-hidden text-tx-2">
                      <span className="truncate">{trace.service}</span>
                      <span className="shrink-0 text-tx-4">·</span>
                      <span className="shrink-0">
                        {t('explore.results.span_count', { count: trace.spans })}
                      </span>
                      {trace.errors > 0 ? (
                        <>
                          <span className="shrink-0 text-tx-4">·</span>
                          <span className="shrink-0 font-semibold text-red-soft">
                            {t('explore.results.error_count', { count: trace.errors })}
                          </span>
                        </>
                      ) : null}
                    </span>
                    <time className="whitespace-nowrap font-mono text-tx-3">
                      {formatTraceStart(trace.startNs, fmt.tz)}
                    </time>
                  </span>
                </button>
              );
            })
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
              className="shrink-0 whitespace-nowrap text-blue-soft hover:underline"
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
  );
}
