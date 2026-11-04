import { useVirtualizer } from '@tanstack/react-virtual';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { ResizeHandle } from '@/shell/ResizeHandle';
import type { SignalReferenceTime } from '@/shell/SignalReference';

import { formatTraceDurationNs } from '../duration';
import { TimeAxis, TimeGrid, useMeasuredWidth, useTimeTicks } from './Axis';
import { durationLabelWidth } from './model';
import { SPAN_ROW_HEIGHT, SpanRow } from './SpanRow';
import type { CriticalSegment } from '../model/criticalPath';
import type { WaterfallRow } from '../model/rows';
import type { TraceTree } from '../model/tree';
import type { TraceViewState } from '../view/state';

const NO_SEGMENTS: readonly CriticalSegment[] = [];

interface WaterfallProps {
  tree: TraceTree;
  state: TraceViewState;
  colors: ReadonlyMap<string, string>;
  time: SignalReferenceTime | undefined;
  /** The width of the column of names, as the reader left it. */
  namesWidth: number;
  /** The spans profile samples kept the id of. */
  profiled?: ReadonlyMap<string, unknown> | undefined;
  onNamesWidthPreview: (width: number) => void;
  onNamesWidthCommit: (width: number) => void;
}

/**
 * The spans of the trace one under another, each a line with its bar on a
 * shared time axis. The lines scroll on their own under the axis; only the
 * lines in sight are drawn, so a trace of thousands of spans scrolls as one
 * of ten. The arrow keys move the selection, open and fold.
 */
export function Waterfall({ tree, state, colors, time, namesWidth, profiled, onNamesWidthPreview, onNamesWidthCommit }: WaterfallProps) {
  const { t } = useTranslation('traces');
  const scrollRef = React.useRef<HTMLDivElement>(null);
  const [frameRef, frameWidth] = useMeasuredWidth<HTMLDivElement>();
  const [axisRef, axisWidth] = useMeasuredWidth<HTMLDivElement>();
  const { rows, selectedId, select, toggle } = state;
  const fromNs = state.zoom?.fromNs ?? 0;
  const toNs = state.zoom?.toNs ?? tree.totalNs;
  const ticks = useTimeTicks(fromNs, toNs, axisWidth);
  const maxNames = Math.max(240, (frameWidth || 900) * 0.6);
  const names = Math.min(namesWidth, maxNames);
  // Room on the right of the time axis for the widest duration of the trace: a bar that reaches the edge is said after it, not over it.
  const gutter = React.useMemo(() => {
    let widest = 0;
    for (const node of tree.nodes) widest = Math.max(widest, formatTraceDurationNs(node.durationNs).length);
    return durationLabelWidth(widest);
  }, [tree]);

  const virtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => SPAN_ROW_HEIGHT,
    overscan: 16,
  });

  const indexOf = React.useCallback((id: string | null) => rows.findIndex((row) => row.node.id === id), [rows]);

  // A span selected from elsewhere, a match or a line of the span list, is brought into sight.
  React.useEffect(() => {
    const index = indexOf(selectedId);
    if (index >= 0) virtualizer.scrollToIndex(index, { align: 'auto' });
    // Only a new selection scrolls; the rows changing under it does not.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selectedId]);

  const focusRow = React.useCallback((id: string) => {
    requestAnimationFrame(() => {
      scrollRef.current
        ?.querySelector<HTMLButtonElement>(`[data-span-id="${CSS.escape(id)}"] [data-testid="trace-span-select"]`)
        ?.focus();
    });
  }, []);

  const moveTo = React.useCallback((row: WaterfallRow | undefined) => {
    if (!row) return;
    select(row.node.id);
    virtualizer.scrollToIndex(indexOf(row.node.id), { align: 'auto' });
    focusRow(row.node.id);
  }, [focusRow, indexOf, select, virtualizer]);

  const onKeyDown = React.useCallback((event: React.KeyboardEvent<HTMLButtonElement>, id: string) => {
    const index = indexOf(id);
    const row = rows[index];
    if (!row) return;

    const keys: Record<string, () => void> = {
      ArrowDown: () => moveTo(rows[index + 1]),
      ArrowUp: () => moveTo(rows[index - 1]),
      Home: () => moveTo(rows[0]),
      End: () => moveTo(rows[rows.length - 1]),
      ArrowRight: () => {
        if (row.hasChildren && row.collapsed) toggle(id);
        else if (row.hasChildren) moveTo(rows[index + 1]);
      },
      ArrowLeft: () => {
        if (row.hasChildren && !row.collapsed) toggle(id);
        else moveTo(rows.find((candidate) => candidate.node.id === row.node.parentId));
      },
    };

    const action = keys[event.key];
    if (!action) return;
    event.preventDefault();
    action();
  }, [indexOf, moveTo, rows, toggle]);

  const tabStopId = indexOf(selectedId) >= 0 ? selectedId : rows[0]?.node.id ?? null;
  const parentService = (row: WaterfallRow) => (row.node.parentId ? tree.byId.get(row.node.parentId)?.service : undefined);
  const { critical } = state;
  // A span the path runs through without time of its own is on it all the same.
  const criticalOf = (id: string) =>
    critical ? critical.segmentsById.get(id) ?? (critical.spans.has(id) ? NO_SEGMENTS : false) : undefined;

  return (
    <div
      ref={frameRef}
      data-testid="trace-waterfall"
      className="relative flex h-full min-h-0 flex-col"
      style={{ '--waterfall-names': `${names}px`, '--waterfall-gutter': `${gutter}px` } as React.CSSProperties}
    >
      <div className="grid h-8 shrink-0 grid-cols-[var(--waterfall-names)_minmax(0,1fr)] overflow-hidden border-b border-bd-0 [scrollbar-gutter:stable]">
        <div className="flex min-w-0 items-center gap-1.5 px-3 font-sans text-xs font-strong text-tx-2">
          <span className="truncate">{t('view.waterfall.column')}</span>
          <span className="font-body tabular-nums text-tx-3">{rows.length}</span>
        </div>
        <div className="relative min-w-0 border-l border-bd-0">
          <div ref={axisRef} className="absolute inset-y-0 left-0" style={{ right: gutter }}>
            <TimeAxis ticks={ticks} fromNs={fromNs} toNs={toNs} />
          </div>
        </div>
      </div>
      <div
        ref={scrollRef}
        data-testid="trace-waterfall-viewport"
        className="relative min-h-0 flex-1 overflow-y-auto overflow-x-hidden [scrollbar-gutter:stable]"
      >
        <div className="relative w-full" style={{ height: virtualizer.getTotalSize() }}>
          <div className="pointer-events-none absolute inset-y-0" style={{ left: names, right: gutter }}>
            <TimeGrid ticks={ticks} fromNs={fromNs} toNs={toNs} />
          </div>
          {virtualizer.getVirtualItems().map((item) => {
            const row = rows[item.index]!;

            return (
              <div key={row.node.id} className="absolute inset-x-0 top-0" style={{ transform: `translateY(${item.start}px)` }}>
                <SpanRow
                  traceId={tree.traceId}
                  row={row}
                  color={colors.get(row.node.service) ?? 'var(--chart-1)'}
                  entersService={parentService(row) !== row.node.service}
                  selected={row.node.id === selectedId}
                  tabStop={row.node.id === tabStopId}
                  fromNs={fromNs}
                  toNs={toNs}
                  traceStartNs={tree.startNs}
                  time={time}
                  critical={criticalOf(row.node.id)}
                  profiled={profiled?.has(row.node.id)}
                  onSelect={select}
                  onToggle={toggle}
                  onKeyDown={onKeyDown}
                />
              </div>
            );
          })}
        </div>
      </div>
      <div className="absolute inset-y-0" style={{ left: names - 4 }}>
        <ResizeHandle
          label={t('view.waterfall.resize_names')}
          width={names}
          min={200}
          max={maxNames}
          grows="right"
          defaultWidth={340}
          onWidthChange={onNamesWidthPreview}
          onWidthCommit={onNamesWidthCommit}
          quiet
          className="h-full"
        />
      </div>
    </div>
  );
}
