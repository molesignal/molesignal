import { ChevronDown, ChevronRight, Circle } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { Span } from '@/api/web';
import { cn } from '@/shell/lib/cn';
import { SignalReference, type SignalReferenceTime } from '@/shell/SignalReference';

import { formatTraceDurationNs } from '../duration';
import { TraceOperationName } from '../TraceOperationName';
import type { SpanNode } from '../types';
import { pct, signalLabelsForSpan, SPAN_SHARE_FILL, spanShare } from './model';

/** The two columns of the waterfall, the same in its head and in every row: the span, and the timeline. */
export const WATERFALL_GRID =
  'grid-cols-[minmax(260px,40%)_minmax(0,1fr)] xl:grid-cols-[minmax(360px,420px)_minmax(0,1fr)]';

/** The mark before a span's name, the same room whether it folds the spans under it or not. */
const MARK = 'mr-1 grid h-5 w-5 shrink-0 place-items-center rounded text-tx-3';

interface WaterfallRowProps {
  traceId: string;
  node: SpanNode;
  /** The span has spans under it, so its mark folds and unfolds them. */
  hasChildren: boolean;
  collapsed: boolean;
  selected: boolean;
  /** The span is one the search found. */
  matched: boolean;
  /** The colour its service is drawn in. */
  serviceColor: string;
  totalNs: number;
  traceStartNs: number;
  time: SignalReferenceTime | undefined;
  /** Where the timeline's guide lines stand, in percent of the trace. */
  ticks: readonly number[];
  onSelect: (span: Span) => void;
  onToggle: (spanId: string) => void;
}

/**
 * A span of the trace: what it is and whose, and when it ran within the trace.
 *
 * The row is not a control itself, so that no control sits inside another. A click anywhere on
 * it selects the span; the keyboard does so from a button that lies under the whole row. The
 * fold mark and the two references are controls of their own, beside that button.
 */
export function WaterfallRow({
  traceId,
  node,
  hasChildren,
  collapsed,
  selected,
  matched,
  serviceColor,
  totalNs,
  traceStartNs,
  time,
  ticks,
  onSelect,
  onToggle,
}: WaterfallRowProps) {
  const { t } = useTranslation('traces');
  const rowId = React.useId();
  const { span } = node;
  const left = pct(node.startOffsetNs, totalNs);
  const width = Math.max(0.35, pct(node.durationNs, totalNs));
  const end = Math.min(100, left + width);
  const durationLabel = formatTraceDurationNs(node.durationNs);
  const failed = span.status === 'ERROR';
  const share = spanShare(node.durationNs, totalNs);
  const shareLabel = t('detail.share_of_trace', { percent: Math.round(pct(node.durationNs, totalNs)) });
  const signalLabels = signalLabelsForSpan(traceId, span);
  const source = { type: 'trace' as const, id: traceId };
  const errorId = `${rowId}error`;
  const matchId = `${rowId}match`;
  // What the row says of the span beside its name: the button that selects it says so too.
  const statusIds = [failed && errorId, matched && matchId].filter(Boolean).join(' ');

  return (
    <div
      onClick={() => onSelect(span)}
      data-testid="trace-span-row"
      className={cn(
        // Every row is the same height, whatever its name is set in.
        'relative grid h-[38px] w-full cursor-pointer border-b border-bd-0 text-left transition-colors hover:bg-bg-2',
        WATERFALL_GRID,
        selected && 'bg-bg-2',
      )}
    >
      {/* The row's size and under its columns: its fill is the row's, and its click is the row's
          own. Under the keyboard a row takes the fill it has under the pointer; the selected row
          has that fill already, so it deepens. */}
      <button
        type="button"
        aria-label={t('detail.inspect_span_aria', {
          operation: span.operation,
          service: span.service,
          duration: durationLabel,
        })}
        aria-describedby={statusIds || undefined}
        aria-pressed={selected}
        data-testid="trace-span-select"
        className={cn('absolute inset-0', selected ? 'focus-visible:bg-bg-3' : 'focus-visible:bg-bg-2')}
      />
      <div className="relative flex min-w-0 items-center border-r border-bd-0 px-3">
        <span style={{ width: node.depth * 16 }} className="shrink-0" />
        {hasChildren ? (
          <button
            type="button"
            aria-label={t('detail.toggle_children')}
            aria-expanded={!collapsed}
            onClick={(event) => {
              // Folding is not selecting: the click stops here.
              event.stopPropagation();
              onToggle(span.span_id);
            }}
            className={cn(
              MARK,
              'hover:bg-bg-3 hover:text-tx-0 focus-visible:bg-bg-3 focus-visible:text-tx-0',
            )}
          >
            {collapsed ? <ChevronRight className="h-3.5 w-3.5" /> : <ChevronDown className="h-3.5 w-3.5" />}
          </button>
        ) : (
          // A span with nothing under it has nothing to fold: its mark is a picture, and a click on it is the row's.
          <span role="img" aria-label={t('detail.leaf_span')} className={MARK}>
            <Circle className="h-2 w-2 fill-current" />
          </span>
        )}
        <span className="mr-2 h-2.5 w-2.5 shrink-0 rounded-sm" style={{ backgroundColor: serviceColor }} />
        <div className="min-w-0 flex-1 font-sans text-xs leading-[16px]">
          <div className="flex h-[16px] min-w-0 items-center">
            <SignalReference
              type="span_id"
              value={span.span_id}
              labelName="span_id"
              labels={signalLabels}
              time={time}
              source={source}
              showIcon={false}
              className="min-w-0 flex-1 justify-start text-left text-tx-0 decoration-current/30 hover:text-tx-0 [&>span]:min-w-0 [&>span]:flex-1"
            >
              <TraceOperationName
                operation={span.operation}
                className="min-w-0 w-full align-bottom font-sans text-xs font-strong leading-[16px] text-tx-0"
              />
            </SignalReference>
          </div>
          <div className="flex h-[16px] min-w-0 items-center gap-2 text-tx-3">
            <SignalReference
              type="service"
              value={span.service}
              labelName="service.name"
              labels={signalLabels}
              time={time}
              source={source}
              showIcon={false}
              className="min-w-0 max-w-full truncate text-tx-3 decoration-current/30 hover:text-indigo-soft [&>span]:truncate"
            >
              {span.service}
            </SignalReference>
            {failed && (
              <span id={errorId} className="text-red">
                {t('detail.span_error')}
              </span>
            )}
            {matched && (
              <span id={matchId} className="text-[var(--blue-ink)]">
                {t('detail.span_match')}
              </span>
            )}
          </div>
        </div>
      </div>
      <div className="relative min-w-0 px-4">
        <div className="relative h-full w-full">
          <div className="absolute inset-y-0 left-0 right-[72px]" data-testid="trace-timeline-track">
            {ticks.map((tick) => (
              <span key={tick} className="absolute top-0 h-full w-px bg-bd-0" style={{ left: `${tick}%` }} />
            ))}
            <div
              role="img"
              aria-label={`${span.operation}, ${span.service}, ${t('explore.table.duration')} ${durationLabel}, ${shareLabel}`}
              className={cn(
                'absolute top-1/2 h-[12px] -translate-y-1/2 cursor-pointer rounded-[3px] transition-[filter] hover:brightness-110',
                failed ? 'bg-red' : SPAN_SHARE_FILL[share],
              )}
              data-testid="trace-span-bar"
              data-tone={failed ? 'error' : share}
              style={{ left: `${left}%`, width: `${width}%` }}
            />
            <span
              aria-label={`${t('explore.table.duration')}: ${durationLabel}`}
              className="pointer-events-none absolute top-1/2 z-[1] ml-1.5 -translate-y-1/2 whitespace-nowrap font-mono text-xs font-medium tabular-nums text-tx-1"
              data-testid="trace-span-duration"
              style={{ left: `${end}%` }}
            >
              {durationLabel}
            </span>
            {span.events.map((event, index) => (
              <span
                key={`${span.span_id}-${event.name}-${index}`}
                className="absolute top-1/2 h-[18px] w-px -translate-y-1/2 bg-tx-0"
                title={event.name}
                style={{ left: `${pct(event.ts_ns - traceStartNs, totalNs)}%` }}
              />
            ))}
          </div>
        </div>
      </div>
    </div>
  );
}
