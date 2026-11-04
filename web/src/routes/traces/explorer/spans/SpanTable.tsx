import type { TFunction } from 'i18next';
import { ArrowDown, ArrowUp } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { SpanListItem, SpanListSort } from '@/api/traceExplorer';
import { useTimeFormatter } from '@/lib/time';
import { gridMinWidth } from '@/routes/apm/frame/gridWidth';
import { cn } from '@/shell/lib/cn';
import { LONG_LIST_ROW_CLASS, longListRowStyle } from '@/shell/list/longList';
import { formatTraceDurationNs } from '@/viz/trace/duration';
import { OUTCOME_EDGE, requestOutcome, spanKindName } from '@/viz/trace/model/span';
import { TraceOperationName } from '@/viz/trace/TraceOperationName';

import { formatTraceStart, HttpStatusCode, OutcomeWord } from '../shared';
import { nextSpanSort, type SpanSortColumn, spanSortDirection } from './model';

/** The tracks of the columns; those it shares with the trace table are as wide as there. */
const TRACKS = ['52px', '48px', '100px', 'minmax(100px,0.9fr)', 'minmax(140px,1.6fr)', '72px', '148px', 'minmax(100px,0.9fr)'];
/** The head and every row are as wide as their columns: past the pane's edge the table scrolls sideways as one. */
const GRID_STYLE = { gridTemplateColumns: TRACKS.join(' '), minWidth: gridMinWidth(TRACKS) };
/** A row is one height, the line under it included: what an undrawn row reserves is what a drawn one takes. */
const ROW_HEIGHT = 32;
const ROW_STYLE = { minHeight: ROW_HEIGHT, ...longListRowStyle(ROW_HEIGHT - 1), ...GRID_STYLE };

/** The key of a span in the list: span ids are only unique within their trace. */
export function spanKey(span: Pick<SpanListItem, 'traceId' | 'spanId'>): string {
  return `${span.traceId}\u0000${span.spanId}`;
}

/**
 * The spans of the page as a table of one line each: how each ended, when it
 * began, its service and operation, its kind, how long it took against the
 * longest of the page, and its trace. A click on a row opens its trace at it.
 */
export function SpanTable({
  spans,
  sort,
  sortEnabled,
  onSortChange,
  openKey,
  onOpen,
  empty,
}: {
  spans: SpanListItem[];
  sort: SpanListSort;
  sortEnabled: boolean;
  onSortChange: (sort: SpanListSort) => void;
  openKey: string | null;
  onOpen: (span: SpanListItem, row: HTMLElement) => void;
  empty?: React.ReactNode;
}) {
  const { t } = useTranslation('traces');
  const fmt = useTimeFormatter();
  const listed = spans.length > 0;
  const longest = React.useMemo(() => Math.max(0, ...spans.map((span) => span.durationNs)), [spans]);
  const listRef = React.useRef<HTMLDivElement>(null);

  const onKeyDown = React.useCallback((event: React.KeyboardEvent<HTMLButtonElement>) => {
    const step = event.key === 'ArrowDown' ? 1 : event.key === 'ArrowUp' ? -1 : 0;
    if (step === 0) return;
    event.preventDefault();
    const rows = [...(listRef.current?.querySelectorAll<HTMLButtonElement>('[data-span-list-row]') ?? [])];
    rows[Math.max(0, Math.min(rows.length - 1, rows.indexOf(event.currentTarget) + step))]?.focus();
  }, []);

  const head = (column: SpanSortColumn | null, label: string) => {
    if (!column || !sortEnabled) return <span className="truncate">{label}</span>;
    const direction = spanSortDirection(column, sort);
    const said = t(direction ? `explore.table.sorted_${direction}` : 'explore.table.sort_by', { column: label });

    return (
      <button
        type="button"
        onClick={() => onSortChange(nextSpanSort(column, sort))}
        aria-label={said}
        title={said}
        data-sort={direction ?? undefined}
        className={cn('inline-flex min-w-0 items-center gap-1 rounded-sm text-left hover:text-tx-0 focus-visible:text-tx-0 focus-visible:outline-none', direction && 'text-tx-0')}
      >
        <span className="truncate">{label}</span>
        {direction === 'descending' && <ArrowDown aria-hidden="true" className="h-3 w-3 shrink-0" />}
        {direction === 'ascending' && <ArrowUp aria-hidden="true" className="h-3 w-3 shrink-0" />}
      </button>
    );
  };

  return (
    <div data-testid="span-table" className="flex min-h-0 flex-1 flex-col">
      {/* The head scrolls with the rows: sideways they move together; down, it stays over them, and a row the keyboard walks to stops below it. */}
      <div ref={listRef} className={cn('min-h-0 scroll-pt-8', listed ? 'flex-1 overflow-auto' : 'shrink-0 overflow-hidden')}>
        <div
          data-span-table-head=""
          className="sticky top-0 z-10 grid h-8 items-center gap-3 border-b border-bd-0 bg-[var(--table-head-surface)] px-3 font-sans text-xs font-strong text-tx-0"
          style={GRID_STYLE}
        >
          {head(null, t('explore.table.columns.status'))}
          {head(null, t('explore.table.columns.http'))}
          {head('start', t('explore.table.columns.start'))}
          {head(null, t('explore.table.columns.service'))}
          {head(null, t('explore.table.columns.operation'))}
          {head(null, t('explore.table.columns.kind'))}
          {head('duration', t('explore.table.columns.duration'))}
          {head(null, t('explore.table.columns.trace'))}
        </div>
        {spans.map((span, index) => (
          <SpanRow
            key={spanKey(span)}
            span={span}
            longest={longest}
            open={spanKey(span) === openKey}
            tabStop={openKey ? spanKey(span) === openKey : index === 0}
            tz={fmt.tz}
            t={t}
            onOpen={onOpen}
            onKeyDown={onKeyDown}
          />
        ))}
      </div>
      {!listed && <div className="min-h-0 flex-1 overflow-auto">{empty}</div>}
    </div>
  );
}

const SpanRow = React.memo(function SpanRow({
  span,
  longest,
  open,
  tabStop,
  tz,
  t,
  onOpen,
  onKeyDown,
}: {
  span: SpanListItem;
  longest: number;
  open: boolean;
  tabStop: boolean;
  tz: string;
  t: TFunction<'traces'>;
  onOpen: (span: SpanListItem, row: HTMLElement) => void;
  onKeyDown: (event: React.KeyboardEvent<HTMLButtonElement>) => void;
}) {
  const outcome = requestOutcome(span.status === 'ERROR', span.httpStatusCode);
  const failed = outcome === 'failed';
  const share = longest > 0 ? Math.max(2, (span.durationNs / longest) * 100) : 0;
  const duration = formatTraceDurationNs(span.durationNs);

  return (
    <button
      type="button"
      data-span-list-row=""
      aria-pressed={open}
      tabIndex={tabStop ? 0 : -1}
      onClick={(event) => onOpen(span, event.currentTarget)}
      onKeyDown={onKeyDown}
      aria-label={t('explore.table.open_span_aria', { operation: span.operation, service: span.service, duration })}
      style={ROW_STYLE}
      className={cn(
        'relative grid w-full items-center gap-3 border-b border-[var(--row-rule)] px-3 text-left font-sans text-xs',
        LONG_LIST_ROW_CLASS,
        open ? 'bg-indigo-dim' : 'hover:bg-bg-2 focus-visible:bg-bg-2 focus-visible:outline-none',
      )}
    >
      <span aria-hidden="true" className={cn('absolute inset-y-0 left-0 w-[2px]', OUTCOME_EDGE[outcome])} />
      <OutcomeWord outcome={outcome} t={t} />
      <HttpStatusCode status={span.httpStatusCode} />
      <span className="truncate tabular-nums text-tx-0">{formatTraceStart(span.startNs, tz)}</span>
      <span className="truncate text-tx-0" title={span.service}>{span.service}</span>
      <TraceOperationName mono={false} operation={span.operation} className="min-w-0 text-tx-0" />
      <span className="truncate text-tx-0">{t(`view.kind.${spanKindName(span.kind)}`)}</span>
      <span className="flex min-w-0 items-center gap-2">
        <span className="w-[58px] shrink-0 text-right tabular-nums text-tx-0">{duration}</span>
        <span aria-hidden="true" className="relative h-1.5 min-w-0 flex-1 overflow-hidden rounded-full bg-bg-3">
          <span className={cn('absolute inset-y-0 left-0 rounded-full', failed ? 'bg-red' : 'bg-[var(--chart-1)]')} style={{ width: `${share}%` }} />
        </span>
      </span>
      <span className="truncate tabular-nums text-tx-0" title={span.traceId}>{span.traceId}</span>
    </button>
  );
});
