import * as React from 'react';
import { createPortal } from 'react-dom';
import { useTranslation } from 'react-i18next';

import { cn } from '@/shell/lib/cn';

import { type FlameNode, formatValue } from './flamebearer';

/** Where the pointer is, in the window. */
export interface FlamePointer {
  x: number;
  y: number;
}

const VIEWPORT_MARGIN = 8;
const POINTER_GAP = 12;

/**
 * What the frame under the pointer costs.
 *
 * It is drawn over the page, not inside the graph. The graph is only as tall
 * as its stack, so a tip laid out in it was taller than the graph and made it
 * scroll: a scrollbar came with every hover and took its width from the bars.
 */
export function FlameTooltip({
  node,
  name,
  note,
  total,
  units,
  diff,
  pointer,
  service,
  container,
}: {
  node: FlameNode;
  /** What the frame is shown as. */
  name: string;
  /** A word on what the name stands for, where it is a sign and not a function. */
  note?: string | undefined;
  total: number;
  units: string;
  diff: boolean;
  pointer: FlamePointer;
  service?: string | undefined;
  /** Where it is drawn when the page is not what is shown: the graph itself while it is full screen. */
  container?: HTMLElement | null | undefined;
}) {
  const { t } = useTranslation('profiles');
  const ref = React.useRef<HTMLDivElement>(null);
  const [position, setPosition] = React.useState<{ left: number; top: number } | null>(null);
  const totalShare = total > 0 ? (node.total / total) * 100 : 0;
  const selfShare = total > 0 ? (node.self / total) * 100 : 0;

  React.useLayoutEffect(() => {
    const element = ref.current;
    if (!element) return;

    const { offsetWidth: width, offsetHeight: height } = element;
    const below = pointer.y + POINTER_GAP;
    const fitsBelow = below + height <= window.innerHeight - VIEWPORT_MARGIN;

    setPosition({
      left: Math.max(
        VIEWPORT_MARGIN,
        Math.min(pointer.x + POINTER_GAP, window.innerWidth - width - VIEWPORT_MARGIN),
      ),
      top: fitsBelow ? below : Math.max(VIEWPORT_MARGIN, pointer.y - POINTER_GAP - height),
    });
    // What it says decides its size, and its size where it fits.
  }, [pointer.x, pointer.y, name, note, diff, service]);

  if (typeof document === 'undefined') return null;

  return createPortal(
    <div
      ref={ref}
      role="tooltip"
      data-flame-tooltip
      className="pointer-events-none fixed z-[70] w-[320px] max-w-[calc(100vw-16px)] rounded-md border-0 bg-[var(--floating-surface)] px-3 py-2.5 shadow-popup"
      // Until it has been measured it has no place to stand.
      style={position ?? { left: 0, top: 0, visibility: 'hidden' }}
    >
      <div className="mb-2 break-all font-mono text-xs font-semibold text-tx-0">
        {name}
        {note ? <span className="ml-2 font-sans font-body text-tx-2">{note}</span> : null}
      </div>
      <div className="grid grid-cols-[1fr_auto] gap-x-4 gap-y-1 font-sans text-xs tabular-nums text-tx-2">
        <span>{t('flamegraph.frame_self')}</span>
        <span className="text-right text-tx-1">
          {formatValue(node.self, units)} · {selfShare.toFixed(1)}%
        </span>
        <span>{t('flamegraph.frame_total')}</span>
        <span className="text-right text-tx-1">
          {formatValue(node.total, units)} · {totalShare.toFixed(1)}%
        </span>
        <span>{t('flamegraph.details.depth')}</span>
        <span className="text-right text-tx-1">{node.depth}</span>
        {service && (
          <>
            <span>{t('flamegraph.details.service')}</span>
            <span className="max-w-[180px] truncate text-right text-tx-1">{service}</span>
          </>
        )}
        {diff && node.delta !== undefined && (
          <>
            <span>Δ</span>
            <span className={cn('text-right', node.delta > 0 ? 'text-red' : node.delta < 0 ? 'text-green' : 'text-tx-1')}>
              {node.delta > 0 ? '+' : ''}
              {formatValue(node.delta, units)}
            </span>
          </>
        )}
      </div>
      <div className="mt-2 border-t border-bd-0 pt-2 font-sans text-xs text-tx-3">
        {t('flamegraph.tooltip.focus_hint')}
      </div>
    </div>,
    container ?? document.body,
  );
}
