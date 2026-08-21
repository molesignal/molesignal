import { X } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type * as webApi from '@/api/web';
import { useTimeFormatter } from '@/lib/time';
import {
  QueryRecoveryState,
  type QueryRecoveryActions,
  type QueryRecoveryCopy,
} from '@/shell/query/RecoveryState';
import type { queryStateFor } from '@/shell/query/State';
import { SignalReference, type SignalReferenceType } from '@/shell/SignalReference';
import { TimezoneSelect } from '@/shell/TimezoneSelect';
import { Button } from '@/shell/ui/button';
import { formatTraceDurationMs } from '@/viz/trace/duration';
import { TraceOperationName } from '@/viz/trace/TraceOperationName';

import { traceFieldValue } from '../fieldPanel/model';
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

interface TraceTableExplorerProps {
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
  selectedTrace: DisplayTrace | null;
  selectedId: string | null;
  visibleFields: TraceFieldName[];
  fieldFilter: string;
  fieldPanelCollapsed: boolean;
  onFieldFilterChange: (value: string) => void;
  onFieldPanelCollapsedChange: (collapsed: boolean) => void;
  onToggleField: (field: TraceFieldName) => void;
  onInsertField: (field: TraceFieldDef) => void;
  onSelectTrace: (id: string) => void;
  onViewSpans: (id: string) => void;
  pagination: TracePaginationModel;
  onSortChange: (sort: webApi.TraceListSort) => void;
}

export function TraceTableExplorer({
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
  selectedTrace,
  selectedId,
  visibleFields,
  fieldFilter,
  fieldPanelCollapsed,
  onFieldFilterChange,
  onFieldPanelCollapsedChange,
  onToggleField,
  onInsertField,
  onSelectTrace,
  onViewSpans,
  pagination,
  onSortChange,
}: TraceTableExplorerProps) {
  const { t } = useTranslation('traces');
  const [tzOverride, setTzOverride] = React.useState('');
  const [summaryOpen, setSummaryOpen] = React.useState(false);
  const fmt = useTimeFormatter({ timezone: tzOverride || undefined });

  const selectTrace = React.useCallback((id: string) => {
    onSelectTrace(id);
    setSummaryOpen(true);
  }, [onSelectTrace]);

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
      <div
        data-workspace-pane="trace-results"
        className="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden rounded-md bg-[var(--functional-surface)] [contain:size]"
      >
        <div className="flex min-h-11 shrink-0 items-center justify-between gap-3 px-3 py-1.5">
          <span className="font-sans text-xs font-semibold text-tx-0">
            {t('explore.results.loaded_count', { count: loadedTraceCount })}
          </span>
          <div className="flex shrink-0 items-center gap-1.5">
            {sortEnabled ? (
              <TraceListSortSelect value={sort} onChange={onSortChange} />
            ) : null}
            <TimezoneSelect value={tzOverride} onChange={setTzOverride} className="h-8" />
          </div>
        </div>
        <div className="grid shrink-0 grid-cols-[minmax(180px,1.4fr)_160px_minmax(180px,1fr)_72px_72px_96px_150px] gap-3 bg-[var(--control-surface)] px-3 py-2 font-sans text-xs font-strong uppercase tracking-normal text-tx-2">
          <span>{t('explore.table.trace')}</span>
          <span>{t('explore.table.service')}</span>
          <span>{t('explore.table.operation')}</span>
          <span>{t('explore.table.spans')}</span>
          <span>{t('explore.table.errors')}</span>
          <span>{t('explore.table.duration')}</span>
          <span>{t('explore.table.start')}</span>
        </div>
        <div className="min-h-0 flex-1 overflow-auto">
          {listState ? (
            <QueryRecoveryState
              state={listState}
              error={listError}
              copy={recoveryCopy}
              recovery={recovery}
              className="h-full min-h-0"
              testId="traces-table-no-data"
            />
          ) : (
            traceList.map((trace) => {
              const selected = selectedId === trace.id;
              return (
                <button
                  key={trace.id}
                  type="button"
                  aria-pressed={selected}
                  onClick={() => selectTrace(trace.id)}
                  className={`grid w-full grid-cols-[minmax(180px,1.4fr)_160px_minmax(180px,1fr)_72px_72px_96px_150px] gap-3 border-b border-bd-0 px-3 py-2 text-left font-sans text-xs hover:bg-bg-2 ${
                    selected ? 'bg-indigo-dim' : 'bg-transparent'
                  }`}
                >
                  <span className="min-w-0">
                    <span className="block truncate font-semibold text-tx-0">{trace.id}</span>
                    <span className="mt-1 block truncate text-xs text-tx-3">{t('explore.results.inspect_hint')}</span>
                  </span>
                  <span className="truncate text-tx-1">{trace.service}</span>
                  <TraceOperationName operation={trace.op} className="text-tx-1" />
                  <span className="text-tx-1">{trace.spans}</span>
                  <span className={trace.errors > 0 ? 'text-red' : 'text-tx-2'}>{trace.errors}</span>
                  <span className="font-semibold text-tx-0">{formatTraceDurationMs(trace.durationMs)}</span>
                  <span className="truncate text-tx-2">{formatTraceStart(trace.startNs, fmt.tz)}</span>
                </button>
              );
            })
          )}
        </div>
        <TracePagination model={pagination} />
      </div>
      {summaryOpen && selectedTrace && (
        <>
          <button
            type="button"
            aria-label={t('explore.results.close_summary')}
            tabIndex={-1}
            onClick={() => setSummaryOpen(false)}
            className="fixed bottom-0 left-0 right-0 top-topbar z-[55] cursor-default border-0 bg-transparent p-0 focus:outline-none"
          />
          <aside
            aria-label={t('explore.results.summary_drawer_aria')}
            className="fixed bottom-0 right-0 top-topbar z-[60] min-h-0 w-[34vw] min-w-[420px] max-w-[660px] bg-bg-0 shadow-drawer data-[state=open]:animate-slide-in-right"
            data-state="open"
          >
            <TraceSummaryPanel
              trace={selectedTrace}
              visibleFields={visibleFields}
              onViewSpans={onViewSpans}
              onClose={() => setSummaryOpen(false)}
            />
          </aside>
        </>
      )}
    </div>
  );
}

function TraceSummaryPanel({
  trace,
  visibleFields,
  onViewSpans,
  onClose,
}: {
  trace: DisplayTrace;
  visibleFields: TraceFieldName[];
  onViewSpans: (id: string) => void;
  onClose: () => void;
}) {
  const { t } = useTranslation('traces');

  return (
    <div className="flex h-full min-h-0 flex-col bg-bg-0">
      <div className="flex items-start gap-3 border-b border-bd-0 px-4 py-3">
        <div className="min-w-0 flex-1">
          <TraceOperationName
            operation={trace.op}
            className="font-sans text-sm font-bold text-tx-0"
          />
          <div className="mt-1 truncate font-sans text-xs text-tx-2">
            <SignalReference type="trace_id" value={trace.id} labels={{ trace_id: trace.id, service: trace.service }}>
              {trace.id}
            </SignalReference>
          </div>
        </div>
        <Button variant="ghost" size="icon" className="h-8 w-8 shrink-0" onClick={onClose} aria-label={t('explore.results.close_summary')}>
          <X className="h-4 w-4" />
        </Button>
      </div>
      <div className="grid grid-cols-2 border-b border-bd-0">
        <TraceSummaryMetric
          label="service"
          value={
            <SignalReference type="service" value={trace.service} labels={{ trace_id: trace.id, service: trace.service }}>
              {trace.service}
            </SignalReference>
          }
        />
        <TraceSummaryMetric label="duration" value={formatTraceDurationMs(trace.durationMs)} />
        <TraceSummaryMetric label="spans" value={String(trace.spans)} />
        <TraceSummaryMetric label="errors" value={String(trace.errors)} {...(trace.errors > 0 ? { tone: 'text-red' } : {})} />
      </div>
      <div className="min-h-0 flex-1 overflow-auto p-4">
        <div className="mb-2 font-sans text-xs font-strong uppercase tracking-normal text-tx-2">{t('explore.results.visible_labels')}</div>
        <div className="flex flex-wrap gap-1.5">
          {visibleFields.map((field) => {
            const value = traceFieldValue(trace, field);
            if (!value) return null;
            const signalType = traceFieldSignalType(field);
            return (
              <span key={field} className="inline-flex max-w-full items-center gap-1 rounded bg-bg-1 px-2 py-1 font-sans text-xs">
                <span className="text-tx-3">{field}</span>
                {signalType ? (
                  <SignalReference
                    type={signalType}
                    value={value}
                    labelName={field}
                    labels={{ trace_id: trace.id, service: trace.service, [field]: value }}
                    className="max-w-[230px] truncate"
                  >
                    {value}
                  </SignalReference>
                ) : (
                  <span className="max-w-[230px] truncate text-tx-1">{value}</span>
                )}
              </span>
            );
          })}
        </div>
      </div>
      <div className="p-3">
        <Button className="w-full" onClick={() => onViewSpans(trace.id)}>
          {t('explore.results.view_spans')}
        </Button>
      </div>
    </div>
  );
}

function TraceSummaryMetric({ label, value, tone }: { label: string; value: React.ReactNode; tone?: string }) {
  return (
    <div className="min-w-0 px-4 py-3">
      <div className="font-sans text-xs font-semibold uppercase tracking-normal text-tx-3">{label}</div>
      <div className={`mt-1 truncate font-sans text-sm font-semibold ${tone ?? 'text-tx-0'}`}>{value}</div>
    </div>
  );
}

function traceFieldSignalType(field: TraceFieldName): SignalReferenceType | null {
  switch (field) {
    case 'trace_id':
      return 'trace_id';
    case 'span_id':
    case 'parent_span_id':
      return 'span_id';
    case 'service.name':
      return 'service';
    default:
      return null;
  }
}
