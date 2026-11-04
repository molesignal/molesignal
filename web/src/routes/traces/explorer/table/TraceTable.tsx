import type { TFunction } from 'i18next';
import { ArrowDown, ArrowUp } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { TraceListSort } from '@/api/web';
import { useTimeFormatter } from '@/lib/time';
import { gridMinWidth } from '@/routes/apm/frame/gridWidth';
import { cn } from '@/shell/lib/cn';
import { LONG_LIST_ROW_CLASS, longListRowStyle } from '@/shell/list/longList';
import { formatTraceDurationMs } from '@/viz/trace/duration';
import { OUTCOME_EDGE, requestOutcome, type RequestOutcome } from '@/viz/trace/model/span';
import { TraceOperationName } from '@/viz/trace/TraceOperationName';

import { formatTraceStart, HttpStatusCode, OutcomeWord } from '../shared';
import type { DisplayTrace } from '../types';
import { columnSortDirection, nextColumnSort, type TraceColumnKey, type TraceTableColumn, traceTableTracks } from './columns';
import { PICK_LETTERS } from './ComparePick';

/** The room on a row's left while traces are picked, for the letter of a picked one: 28px, and the right's own 12px. */
const PICK_ROOM = 'pl-8';
const PICK_PADDING_PX = 40;

type PickLetter = (typeof PICK_LETTERS)[number];

function pickLetter(picked: readonly string[], id: string): PickLetter | null {
  const index = picked.indexOf(id);
  return index >= 0 ? PICK_LETTERS[index] ?? null : null;
}

/** A row is one height, the line under it included, as a log line is: what an undrawn row reserves is what a drawn one takes. */
const ROW_HEIGHT = 32;
const ROW_STYLE = { minHeight: ROW_HEIGHT, ...longListRowStyle(ROW_HEIGHT - 1) };

interface TraceTableProps {
  traces: DisplayTrace[];
  /** The columns as the reader arranged them: ones the trace carries, and fields of its root span. */
  columns: readonly TraceTableColumn[];
  sort: TraceListSort;
  /** The order can be changed: a statement keeps the order it wrote. */
  sortEnabled: boolean;
  onSortChange: (sort: TraceListSort) => void;
  /** The trace open in the drawer. */
  openId: string | null;
  onOpen: (trace: DisplayTrace, row: HTMLElement) => void;
  /**
   * While two traces are picked to compare, the ones picked, in order: a row
   * then picks its trace instead of opening it. `null` the rest of the time.
   */
  picked?: readonly string[] | null | undefined;
  /** What stands in the rows' place when there are none to show. */
  empty?: React.ReactNode;
}

/**
 * The traces of the page as a table of one line each, in the columns the
 * reader arranged: how each ended, when it began, where, what it did, how
 * long it took against the longest of the page, how big it was, its id, and
 * fields of its root span. A click on a column sorts by it; a click on a row
 * opens the trace.
 */
export function TraceTable({ traces, columns, sort, sortEnabled, onSortChange, openId, onOpen, picked = null, empty }: TraceTableProps) {
  const { t } = useTranslation('traces');
  const fmt = useTimeFormatter();
  const tracks = traceTableTracks(columns);
  const grid = tracks.join(' ');
  const picking = picked !== null;
  // The head and every row are as wide as their columns: past the pane's edge the table scrolls sideways as one.
  // While traces are picked, each row keeps room on its left for the letter of a picked one.
  const minWidth = gridMinWidth(tracks, 12, picking ? PICK_PADDING_PX : 24);
  const listed = traces.length > 0;
  const longest = React.useMemo(() => Math.max(0, ...traces.map((trace) => trace.durationMs)), [traces]);
  const listRef = React.useRef<HTMLDivElement>(null);

  // One stop in the tab order: the arrows walk the rows from there.
  const onKeyDown = React.useCallback((event: React.KeyboardEvent<HTMLButtonElement>) => {
    const step = event.key === 'ArrowDown' ? 1 : event.key === 'ArrowUp' ? -1 : 0;
    if (step === 0) return;
    event.preventDefault();
    const rows = [...(listRef.current?.querySelectorAll<HTMLButtonElement>('[data-trace-row]') ?? [])];
    const at = rows.indexOf(event.currentTarget);
    rows[Math.max(0, Math.min(rows.length - 1, at + step))]?.focus();
  }, []);

  return (
    <div data-testid="trace-table" className="flex min-h-0 flex-1 flex-col">
      {/* The head scrolls with the rows: sideways they move together; down, it stays over them, and a row the keyboard walks to stops below it. */}
      <div ref={listRef} className={cn('min-h-0 scroll-pt-8', listed ? 'flex-1 overflow-auto' : 'shrink-0 overflow-hidden')}>
        <div
          data-trace-table-head=""
          className={cn('sticky top-0 z-10 grid h-8 items-center gap-3 border-b border-bd-0 bg-[var(--table-head-surface)] px-3 font-sans text-xs font-strong text-tx-0', picking && PICK_ROOM)}
          style={{ gridTemplateColumns: grid, minWidth }}
        >
          {columns.map((column) =>
            column.key ? (
              <HeadCell key={column.id} column={column.key} sort={sort} sortEnabled={sortEnabled} onSortChange={onSortChange} t={t} />
            ) : (
              <span key={column.id} className="truncate" title={column.id}>
                {column.id}
              </span>
            ),
          )}
        </div>
        {traces.map((trace, index) => (
          <TraceTableRow
            key={trace.id}
            trace={trace}
            grid={grid}
            minWidth={minWidth}
            columns={columns}
            longest={longest}
            open={trace.id === openId}
            pick={picking ? pickLetter(picked, trace.id) : undefined}
            tabStop={openId ? trace.id === openId : index === 0}
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

function HeadCell({
  column,
  sort,
  sortEnabled,
  onSortChange,
  t,
}: {
  column: TraceColumnKey;
  sort: TraceListSort;
  sortEnabled: boolean;
  onSortChange: (sort: TraceListSort) => void;
  t: TFunction<'traces'>;
}) {
  const next = sortEnabled ? nextColumnSort(column, sort) : null;
  const direction = sortEnabled ? columnSortDirection(column, sort) : null;
  const label = t(`explore.table.columns.${column}`);
  const right = column === 'spans' || column === 'errors';

  if (!next) {
    return <span className={cn('truncate', right && 'text-right')}>{label}</span>;
  }

  return (
    <span data-sort={direction ?? undefined} className={cn('min-w-0', right && 'text-right')}>
      <button
        type="button"
        onClick={() => onSortChange(next)}
        aria-label={t(direction ? `explore.table.sorted_${direction}` : 'explore.table.sort_by', { column: label })}
        className={cn(
          'relative inline-flex max-w-full items-center gap-1 rounded-sm text-left after:absolute after:-inset-y-[3px] after:inset-x-0 after:content-[""] hover:text-tx-0 focus-visible:text-tx-0 focus-visible:outline-none',
          direction && 'text-tx-0',
        )}
        title={t(direction ? `explore.table.sorted_${direction}` : 'explore.table.sort_by', { column: label })}
      >
        <span className="truncate">{label}</span>
        {direction === 'descending' && <ArrowDown aria-hidden="true" className="h-3 w-3 shrink-0" />}
        {direction === 'ascending' && <ArrowUp aria-hidden="true" className="h-3 w-3 shrink-0" />}
      </button>
    </span>
  );
}

/** A trace of the table, drawn again only when it changes itself: a page can hold thousands. */
const TraceTableRow = React.memo(function TraceTableRow({
  trace,
  grid,
  minWidth,
  columns,
  longest,
  open,
  pick,
  tabStop,
  tz,
  t,
  onOpen,
  onKeyDown,
}: {
  trace: DisplayTrace;
  grid: string;
  minWidth: string;
  columns: readonly TraceTableColumn[];
  longest: number;
  open: boolean;
  /** While traces are picked: the letter this one stands under, `null` for one not picked; `undefined` the rest of the time. */
  pick: PickLetter | null | undefined;
  tabStop: boolean;
  tz: string;
  t: TFunction<'traces'>;
  onOpen: (trace: DisplayTrace, row: HTMLElement) => void;
  onKeyDown: (event: React.KeyboardEvent<HTMLButtonElement>) => void;
}) {
  const outcome = requestOutcome(trace.errors > 0 || trace.status === 'ERROR', trace.httpStatus);
  const share = longest > 0 ? Math.max(2, (trace.durationMs / longest) * 100) : 0;
  const picking = pick !== undefined;
  const pressed = picking ? pick !== null : open;
  const label = { operation: trace.op, service: trace.service, duration: formatTraceDurationMs(trace.durationMs) };

  return (
    <button
      type="button"
      data-trace-row=""
      data-trace-id={trace.id}
      data-pick={pick ?? undefined}
      aria-pressed={pressed}
      tabIndex={tabStop ? 0 : -1}
      onClick={(event) => onOpen(trace, event.currentTarget)}
      onKeyDown={onKeyDown}
      aria-label={t(picking ? 'explore.compare.pick_aria' : 'explore.table.open_aria', label)}
      style={{ ...ROW_STYLE, gridTemplateColumns: grid, minWidth }}
      className={cn(
        'relative grid w-full items-center gap-3 border-b border-[var(--row-rule)] px-3 text-left font-sans text-xs',
        LONG_LIST_ROW_CLASS,
        picking && PICK_ROOM,
        pressed ? 'bg-indigo-dim' : 'hover:bg-bg-2 focus-visible:bg-bg-2 focus-visible:outline-none',
      )}
    >
      {/* How the trace ended, at a glance down the left edge. */}
      <span aria-hidden="true" className={cn('absolute inset-y-0 left-0 w-[2px]', OUTCOME_EDGE[outcome])} />
      {picking && (
        <span
          aria-hidden="true"
          className={cn(
            'absolute left-[9px] top-1/2 grid h-4 w-4 -translate-y-1/2 place-items-center rounded-sm type-micro font-strong leading-none',
            pick ? 'bg-indigo text-white' : 'bg-bg-3',
          )}
        >
          {pick}
        </span>
      )}
      {columns.map((column) => (
        <TraceCell key={column.id} column={column} trace={trace} outcome={outcome} share={share} tz={tz} t={t} />
      ))}
    </button>
  );
});

function TraceCell({
  column,
  trace,
  outcome,
  share,
  tz,
  t,
}: {
  column: TraceTableColumn;
  trace: DisplayTrace;
  outcome: RequestOutcome;
  share: number;
  tz: string;
  t: TFunction<'traces'>;
}) {
  switch (column.key) {
    case 'status':
      return <OutcomeWord outcome={outcome} t={t} />;
    case 'http':
      return <HttpStatusCode status={trace.httpStatus} />;
    case 'start':
      return <span className="truncate tabular-nums text-tx-0">{formatTraceStart(trace.startNs, tz)}</span>;
    case 'service':
      return <span className="truncate text-tx-0" title={trace.service}>{trace.service}</span>;
    case 'operation':
      return <TraceOperationName mono={false} operation={trace.op} className="min-w-0 text-tx-0" />;
    case 'duration':
      return (
        <span className="flex min-w-0 items-center gap-2">
          <span className="w-[58px] shrink-0 text-right tabular-nums text-tx-0">{formatTraceDurationMs(trace.durationMs)}</span>
          <span aria-hidden="true" className="relative h-1.5 min-w-0 flex-1 overflow-hidden rounded-full bg-bg-3">
            <span className={cn('absolute inset-y-0 left-0 rounded-full', outcome === 'failed' ? 'bg-red' : 'bg-[var(--chart-1)]')} style={{ width: `${share}%` }} />
          </span>
        </span>
      );
    case 'spans':
      return <span className="text-right tabular-nums text-tx-0">{trace.spans}</span>;
    case 'errors':
      return <span className={cn('text-right tabular-nums', trace.errors > 0 ? 'font-strong text-[var(--red-ink)]' : 'text-tx-0')}>{trace.errors}</span>;
    case 'trace':
      return <span className="truncate tabular-nums text-tx-0" title={trace.id}>{trace.id}</span>;
    default: {
      const value = trace.root?.[column.id];
      const text = value === undefined || value === null ? '' : typeof value === 'string' ? value : JSON.stringify(value);
      return (
        <span className="truncate text-tx-0" title={text}>
          {text || <span className="text-tx-4">—</span>}
        </span>
      );
    }
  }
}
