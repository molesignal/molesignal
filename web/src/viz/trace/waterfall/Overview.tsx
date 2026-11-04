import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { cn } from '@/shell/lib/cn';

import type { TraceNode, TraceTree } from '../model/tree';
import type { TraceZoom } from '../view/state';

/** The most spans the strip draws: past that, the critical path while it is drawn, every failed span and the longest. */
const DRAWN_SPANS = 1_500;
/** The depths the strip has room for; deeper spans share the last. */
const LEVELS = 8;
const HEIGHT = 34;
/** A drag shorter than this is a click. */
const LEAST_DRAG_PX = 4;

/**
 * The whole trace at a glance, every span a stroke in its service's colour,
 * at its depth: the trace's shape before any line of it is read. Dragging
 * across it narrows the time axis to that stretch; a double click widens it
 * back to the whole trace.
 */
export function TraceOverview({
  tree,
  colors,
  zoom,
  onZoom,
  critical = null,
  className,
}: {
  tree: TraceTree;
  colors: ReadonlyMap<string, string>;
  zoom: TraceZoom | null;
  onZoom: (zoom: TraceZoom | null) => void;
  /** The spans of the critical path, while it is drawn: the rest are drawn faint. */
  critical?: ReadonlySet<string> | null | undefined;
  className?: string | undefined;
}) {
  const { t } = useTranslation('traces');
  const stripRef = React.useRef<HTMLDivElement>(null);
  const [drag, setDrag] = React.useState<{ from: number; to: number } | null>(null);
  const total = tree.totalNs;

  const drawn = React.useMemo(() => {
    if (tree.nodes.length <= DRAWN_SPANS) return tree.nodes;
    const kept = (node: TraceNode) => Number(critical?.has(node.id) ?? false);
    return [...tree.nodes]
      .sort((left, right) => kept(right) - kept(left) || Number(right.failed) - Number(left.failed) || right.durationNs - left.durationNs)
      .slice(0, DRAWN_SPANS);
  }, [critical, tree]);

  const share = (clientX: number) => {
    const box = stripRef.current?.getBoundingClientRect();
    if (!box || box.width === 0) return 0;
    return Math.min(1, Math.max(0, (clientX - box.left) / box.width));
  };

  const begin = (event: React.PointerEvent<HTMLDivElement>) => {
    if (event.button !== 0) return;
    event.preventDefault();
    const box = stripRef.current?.getBoundingClientRect();
    const from = share(event.clientX);
    setDrag({ from, to: from });

    const move = (pointer: PointerEvent) => setDrag({ from, to: share(pointer.clientX) });
    const end = (pointer: PointerEvent) => {
      globalThis.removeEventListener('pointermove', move);
      globalThis.removeEventListener('pointerup', end);
      setDrag(null);

      const to = share(pointer.clientX);
      if (!box || Math.abs(to - from) * box.width < LEAST_DRAG_PX) return;
      onZoom({ fromNs: Math.min(from, to) * total, toNs: Math.max(from, to) * total });
    };

    globalThis.addEventListener('pointermove', move);
    globalThis.addEventListener('pointerup', end);
  };

  const windowStart = zoom ? zoom.fromNs / total : 0;
  const windowEnd = zoom ? zoom.toNs / total : 1;

  return (
    <div
      ref={stripRef}
      role="img"
      aria-label={t('view.overview.aria')}
      title={t('view.overview.hint')}
      data-testid="trace-overview"
      onPointerDown={begin}
      onDoubleClick={() => onZoom(null)}
      className={cn('relative cursor-crosshair touch-none select-none overflow-hidden rounded-sm bg-bg-1', className)}
      style={{ height: HEIGHT }}
    >
      <svg viewBox={`0 0 1000 ${HEIGHT}`} preserveAspectRatio="none" className="absolute inset-0 h-full w-full">
        {drawn.map((node) => (
          <rect
            key={node.id}
            x={(node.offsetNs / total) * 1000}
            width={Math.max(0.8, (node.durationNs / total) * 1000)}
            y={3 + Math.min(node.depth, LEVELS - 1) * 3.75}
            height={2.75}
            fill={node.failed ? 'var(--red)' : colors.get(node.service) ?? 'var(--chart-1)'}
            opacity={critical && !critical.has(node.id) ? 0.3 : undefined}
          />
        ))}
      </svg>
      {zoom && (
        <>
          <div className="absolute inset-y-0 left-0 bg-bg-0/70" style={{ width: `${windowStart * 100}%` }} />
          <div className="absolute inset-y-0 right-0 bg-bg-0/70" style={{ width: `${(1 - windowEnd) * 100}%` }} />
          <div
            data-testid="trace-overview-window"
            className="absolute inset-y-0 border-x-2 border-indigo"
            style={{ left: `${windowStart * 100}%`, width: `${(windowEnd - windowStart) * 100}%` }}
          />
        </>
      )}
      {drag && (
        <div
          className="absolute inset-y-0 bg-indigo/20"
          style={{ left: `${Math.min(drag.from, drag.to) * 100}%`, width: `${Math.abs(drag.to - drag.from) * 100}%` }}
        />
      )}
    </div>
  );
}
