import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { cn } from '@/shell/lib/cn';

import { formatTraceDurationNs } from '../duration';
import { percentAcross } from '../model/axis';
import type { TraceNode, TraceTree } from '../model/tree';
import { TraceOperationName } from '../TraceOperationName';
import type { TraceViewState } from '../view/state';
import { TimeAxis, TimeGrid, useMeasuredWidth, useTimeTicks } from '../waterfall/Axis';

const LEVEL_HEIGHT = 22;
/** A block narrower than this says nothing at this zoom: it is left out until the reader zooms in. */
const LEAST_BLOCK_PX = 0.5;
/** A block this wide names its span. */
const LABELLED_PX = 56;

/**
 * The trace as a flame graph: every span a block under the span that called
 * it, as long as it ran, in its service's colour. Where the time went reads
 * at a glance. A click selects a span; a double click narrows the time axis
 * to it.
 */
export function FlameGraph({
  tree,
  state,
  colors,
}: {
  tree: TraceTree;
  state: TraceViewState;
  colors: ReadonlyMap<string, string>;
}) {
  const { t } = useTranslation('traces');
  const [axisRef, width] = useMeasuredWidth<HTMLDivElement>();
  const fromNs = state.zoom?.fromNs ?? 0;
  const toNs = state.zoom?.toNs ?? tree.totalNs;
  const ticks = useTimeTicks(fromNs, toNs, width);
  const spanNs = Math.max(1, toNs - fromNs);
  const focus = state.focusId ? tree.byId.get(state.focusId) : undefined;
  const baseDepth = focus?.depth ?? 0;

  // Failed spans and every span above one, read once from the deepest up.
  const failedPaths = React.useMemo(() => {
    const kept = new Set<string>();
    for (let index = tree.nodes.length - 1; index >= 0; index -= 1) {
      const node = tree.nodes[index]!;
      if (node.failed || node.children.some((child) => kept.has(child.id))) kept.add(node.id);
    }
    return kept;
  }, [tree.nodes]);

  const levels = React.useMemo(() => {
    const byLevel: TraceNode[][] = [];
    const inside = focus ? new Set(collect(focus)) : null;

    for (const node of tree.nodes) {
      if (inside && !inside.has(node.id)) continue;
      if (state.errorsOnly && !failedPaths.has(node.id)) continue;

      const visible = node.offsetNs + node.durationNs >= fromNs && node.offsetNs <= toNs;
      const px = (node.durationNs / spanNs) * (width || 800);
      if (!visible || (px < LEAST_BLOCK_PX && !node.failed)) continue;

      const level = node.depth - baseDepth;
      (byLevel[level] ??= []).push(node);
    }

    return byLevel;
  }, [baseDepth, failedPaths, focus, fromNs, spanNs, state.errorsOnly, toNs, tree.nodes, width]);

  return (
    <div data-testid="trace-flame" className="flex h-full min-h-0 flex-col">
      <div className="h-8 shrink-0 border-b border-bd-0 px-3">
        <div ref={axisRef} className="relative h-full">
          <TimeAxis ticks={ticks} fromNs={fromNs} toNs={toNs} />
        </div>
      </div>
      <div className="relative min-h-0 flex-1 overflow-auto px-3 py-2">
        <div className="relative" style={{ height: Math.max(1, levels.length) * LEVEL_HEIGHT }}>
          <TimeGrid ticks={ticks} fromNs={fromNs} toNs={toNs} />
          {levels.map((nodes, level) =>
            nodes.map((node) => {
              const left = percentAcross(node.offsetNs, fromNs, toNs);
              const right = percentAcross(node.offsetNs + node.durationNs, fromNs, toNs);
              const px = ((right - left) / 100) * (width || 800);
              const selected = node.id === state.selectedId;
              const matched = state.matchSet?.has(node.id) ?? false;
              const offPath = state.critical !== null && !state.critical.spans.has(node.id);

              return (
                <button
                  key={node.id}
                  type="button"
                  data-testid="trace-flame-block"
                  data-span-id={node.id}
                  data-critical={state.critical ? String(!offPath) : undefined}
                  aria-pressed={selected}
                  aria-label={t('view.row.select_aria', {
                    operation: node.span.operation,
                    service: node.service,
                    duration: formatTraceDurationNs(node.durationNs),
                  })}
                  title={`${node.service} · ${node.span.operation} · ${formatTraceDurationNs(node.durationNs)}`}
                  onClick={() => state.select(node.id)}
                  onDoubleClick={() => state.setZoom({ fromNs: node.offsetNs, toNs: node.offsetNs + node.durationNs })}
                  className={cn(
                    'absolute flex items-center overflow-hidden rounded-[3px] px-1.5 text-left font-sans text-xs text-white',
                    'transition-[filter] hover:brightness-110 focus-visible:outline-none focus-visible:brightness-125',
                    selected && 'font-strong',
                    node.failed && 'bg-red',
                    ((state.matchSet && !matched) || offPath) && !selected && 'opacity-40',
                  )}
                  style={{
                    top: level * LEVEL_HEIGHT,
                    height: LEVEL_HEIGHT - 2,
                    left: `${left}%`,
                    width: `max(1px, calc(${right - left}% - 1px))`,
                    // The selected span keeps its service's colour and is drawn round, in the colour of the text.
                    ...(selected ? { boxShadow: 'inset 0 0 0 2px var(--tx-0)' } : {}),
                    ...(node.failed ? {} : { backgroundColor: colors.get(node.service) ?? 'var(--chart-1)' }),
                  }}
                >
                  {px >= LABELLED_PX && (
                    <TraceOperationName operation={node.span.operation} className="min-w-0 truncate text-white [&_*]:text-white" />
                  )}
                </button>
              );
            }),
          )}
        </div>
      </div>
    </div>
  );
}

function collect(node: TraceNode): string[] {
  const ids: string[] = [];
  const stack = [node];

  while (stack.length > 0) {
    const next = stack.pop()!;
    ids.push(next.id);
    stack.push(...next.children);
  }

  return ids;
}
