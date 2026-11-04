import { ChevronDown, ChevronRight, CircleAlert, Flame, Link2 } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { cn } from '@/shell/lib/cn';
import { isNestedControlClick } from '@/shell/lib/nestedControl';
import { SignalReference, type SignalReferenceTime } from '@/shell/SignalReference';

import { formatTraceDurationNs } from '../duration';
import { QUIET_REFERENCE, signalLabelsForSpan } from './model';
import { percentAcross } from '../model/axis';
import type { CriticalSegment } from '../model/criticalPath';
import type { WaterfallRow } from '../model/rows';
import { HTTP_STATUS_TEXT, httpStatusOf, httpStatusTone } from '../model/span';
import { TraceOperationName } from '../TraceOperationName';

/** Every line is one height, whatever its name is set in. */
export const SPAN_ROW_HEIGHT = 32;
/** How far a span stands in from its caller. */
const INDENT = 14;

interface SpanRowProps {
  traceId: string;
  row: WaterfallRow;
  /** The colour of the span's service. */
  color: string;
  /** The trace enters the span's service here: the line names it. */
  entersService: boolean;
  selected: boolean;
  /** The line the arrow keys move from: the one stop of the list in the tab order. */
  tabStop: boolean;
  /** The stretch of the trace in view, as offsets from its start. */
  fromNs: number;
  toNs: number;
  traceStartNs: number;
  time: SignalReferenceTime | undefined;
  /**
   * Where the span stands on the critical path while it is drawn: `false` off
   * it, its stretches on it otherwise; `undefined` while it is not drawn.
   */
  critical?: readonly CriticalSegment[] | false | undefined;
  /** Profile samples kept the span's id. */
  profiled?: boolean | undefined;
  onSelect: (id: string) => void;
  onToggle: (id: string) => void;
  onKeyDown: (event: React.KeyboardEvent<HTMLButtonElement>, id: string) => void;
}

/**
 * A span of the waterfall: what it is and whose, and when it ran within the
 * part of the trace in view. A click anywhere on the line selects it; the
 * keyboard does so from the button under the whole line. The fold mark and
 * the references are controls of their own: the line leaves a click on one
 * of them be, and lets it reach the page, so an open menu closes.
 */
export const SpanRow = React.memo(function SpanRow({
  traceId,
  row,
  color,
  entersService,
  selected,
  tabStop,
  fromNs,
  toNs,
  traceStartNs,
  time,
  critical,
  profiled = false,
  onSelect,
  onToggle,
  onKeyDown,
}: SpanRowProps) {
  const { t } = useTranslation('traces');
  const { node } = row;
  const { span } = node;
  const status = httpStatusOf(span);
  const duration = formatTraceDurationNs(node.durationNs);
  const start = percentAcross(node.offsetNs, fromNs, toNs);
  const end = percentAcross(node.offsetNs + node.durationNs, fromNs, toNs);
  const inView = node.offsetNs + node.durationNs >= fromNs && node.offsetNs <= toNs;
  const labels = signalLabelsForSpan(traceId, span);
  const source = { type: 'trace' as const, id: traceId };
  const statusId = React.useId();
  const onPath = critical !== undefined && critical !== false;
  const faint = row.path || critical === false;

  return (
    <div
      data-testid="trace-span-row"
      data-span-id={node.id}
      data-path={row.path || undefined}
      data-critical={critical === undefined ? undefined : String(onPath)}
      onClick={(event) => {
        if (!isNestedControlClick(event)) onSelect(node.id);
      }}
      style={{ height: SPAN_ROW_HEIGHT }}
      className={cn(
        'group relative grid w-full cursor-pointer grid-cols-[var(--waterfall-names)_minmax(0,1fr)] border-b border-bd-0 text-left',
        selected ? 'bg-indigo-dim' : 'hover:bg-bg-2',
        row.path && 'text-tx-3',
      )}
    >
      <button
        type="button"
        tabIndex={tabStop ? 0 : -1}
        aria-label={t('view.row.select_aria', { operation: span.operation, service: span.service, duration })}
        aria-describedby={statusId}
        aria-pressed={selected}
        onClick={() => onSelect(node.id)}
        onKeyDown={(event) => onKeyDown(event, node.id)}
        data-testid="trace-span-select"
        className={cn('absolute inset-0 focus-visible:outline-none', selected ? 'focus-visible:bg-indigo/20' : 'focus-visible:bg-bg-3')}
      />
      <div
        className="relative flex min-w-0 items-center gap-1 pr-2"
        style={{
          paddingLeft: 8 + row.depth * INDENT,
          // One quiet line for each level above: the way down reads without boxes.
          backgroundImage: row.depth > 0 ? 'repeating-linear-gradient(to right, var(--bd-0) 0 1px, transparent 1px 14px)' : undefined,
          backgroundSize: `${row.depth * INDENT}px 100%`,
          backgroundPosition: '15px 0',
          backgroundRepeat: 'no-repeat',
        }}
      >
        {row.hasChildren ? (
          <button
            type="button"
            tabIndex={-1}
            aria-label={t(row.collapsed ? 'view.row.expand' : 'view.row.collapse')}
            aria-expanded={!row.collapsed}
            onClick={() => onToggle(node.id)}
            className="relative grid h-5 w-5 shrink-0 place-items-center rounded text-tx-3 hover:bg-bg-3 hover:text-tx-0"
          >
            {row.collapsed ? <ChevronRight className="h-3.5 w-3.5" /> : <ChevronDown className="h-3.5 w-3.5" />}
          </button>
        ) : (
          <span aria-hidden="true" className="h-5 w-5 shrink-0" />
        )}
        {row.hasChildren && (
          <span className="relative shrink-0 font-sans text-xs tabular-nums text-tx-3" title={t('view.row.descendants', { count: node.descendants })}>
            {node.descendants}
          </span>
        )}
        <span aria-hidden="true" className="relative ml-0.5 h-2 w-2 shrink-0 rounded-sm" style={{ backgroundColor: color }} />
        {entersService && (
          <span data-span-service="" className="relative min-w-0 max-w-[40%] shrink-[2] truncate font-sans text-xs font-strong text-tx-1">{node.service}</span>
        )}
        <SignalReference
          type="span_id"
          value={span.spanId}
          labelName="span_id"
          labels={labels}
          time={time}
          source={source}
          showIcon={false}
          tabIndex={-1}
          onActivate={() => onSelect(node.id)}
          className={cn('relative min-w-0 flex-1 justify-start text-left text-tx-0 [&>span]:min-w-0 [&>span]:flex-1', QUIET_REFERENCE)}
        >
          <TraceOperationName operation={span.operation} className="w-full min-w-0 font-sans text-xs font-body leading-[16px] text-tx-0" />
        </SignalReference>
        <span id={statusId} className="relative flex shrink-0 items-center gap-1">
          {onPath && <span className="sr-only">{t('view.row.critical')}</span>}
          {status !== null && (
            <span className={cn('type-micro font-sans font-strong leading-4 tabular-nums', HTTP_STATUS_TEXT[httpStatusTone(status)])}>
              {status}
            </span>
          )}
          {node.failed && (
            <CircleAlert aria-label={t('view.row.failed')} role="img" className="h-3.5 w-3.5 text-[var(--red-ink)]" />
          )}
          {span.links.length > 0 && (
            <Link2 aria-label={t('view.row.links', { count: span.links.length })} role="img" className="h-3.5 w-3.5 text-tx-3" />
          )}
          {profiled && <Flame aria-label={t('view.row.profiled')} role="img" className="h-3.5 w-3.5 text-tx-3" data-testid="trace-span-profiled" />}
        </span>
      </div>
      <div className="relative min-w-0 border-l border-bd-0">
        {/* The time axis's stretch: short of the room on the right, where the duration of a bar that ends at the edge is written. */}
        <div data-span-track="" className="absolute inset-y-0 left-0" style={{ right: 'var(--waterfall-gutter, 0px)' }}>
          {inView && (
            <>
              <div
                data-testid="trace-span-bar"
                data-tone={node.failed ? 'error' : 'service'}
                className={cn('absolute top-1/2 h-[12px] -translate-y-1/2 rounded-[3px]', node.failed && 'bg-red')}
                style={{
                  left: `${start}%`,
                  width: `max(2px, ${end - start}%)`,
                  ...(node.failed ? {} : { backgroundColor: color }),
                  opacity: faint ? (critical === false ? 0.3 : 0.45) : 1,
                }}
              >
                {/* The span's own stretches of the critical path: a line of the ink along the bar. */}
                {onPath &&
                  critical.map((segment) => (
                    <span
                      key={segment.fromNs}
                      data-testid="trace-critical-segment"
                      aria-hidden="true"
                      className="absolute top-1/2 h-[4px] -translate-y-1/2 rounded-full bg-tx-0"
                      style={{
                        left: `${percentAcross(segment.fromNs - node.startNs, 0, node.durationNs)}%`,
                        width: `max(2px, ${percentAcross(segment.toNs - node.startNs, 0, node.durationNs) - percentAcross(segment.fromNs - node.startNs, 0, node.durationNs)}%)`,
                      }}
                    />
                  ))}
                {span.events.map((event, index) => (
                  <span
                    key={`${event.name}-${index}`}
                    title={event.name}
                    className="absolute top-[-3px] h-[18px] w-px bg-tx-0"
                    style={{ left: `${percentAcross(event.tsNs - traceStartNs - node.offsetNs, 0, node.durationNs)}%` }}
                  />
                ))}
              </div>
              {/* Past the end of its bar, never over it: the room on the right holds the duration of a bar that reaches the edge. */}
              <span
                data-testid="trace-span-duration"
                className="pointer-events-none absolute top-1/2 -translate-y-1/2 whitespace-nowrap px-1.5 font-sans text-xs tabular-nums text-tx-1"
                style={{ left: `${end}%` }}
              >
                {duration}
              </span>
            </>
          )}
        </div>
      </div>
    </div>
  );
});
