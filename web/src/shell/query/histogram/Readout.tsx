import * as React from 'react';
import { createPortal } from 'react-dom';
import { useTranslation } from 'react-i18next';

import { cn } from '@/shell/lib/cn';

/** Where the readout stands, in the window: over the middle of a bar, above the plot or under the module. */
export interface ReadoutAnchor {
  x: number;
  above: number;
  below: number;
}

export interface ReadoutRow {
  name: string;
  /** The fill the row's bars are drawn in. */
  fill: string;
  count: number;
}

const VIEWPORT_MARGIN = 8;
const GAP = 6;

/**
 * What the bar under the pointer holds: the time it spans, how many fell in
 * it, and how they divide, where they are divided. It stands above the plot,
 * so it never lies on a bar; where there is no room above, it goes under the
 * module.
 */
export function HistogramReadout({
  anchor,
  range,
  total,
  rows,
}: {
  anchor: ReadoutAnchor;
  range: string;
  total: number;
  rows: ReadoutRow[];
}) {
  const { i18n } = useTranslation();
  const ref = React.useRef<HTMLDivElement>(null);
  const [position, setPosition] = React.useState<{ left: number; top: number } | null>(null);
  // The axis rounds; the readout is where a count is read exactly.
  const exact = (count: number) => count.toLocaleString(i18n.resolvedLanguage ?? i18n.language);

  React.useLayoutEffect(() => {
    const element = ref.current;
    if (!element) return;

    const { offsetWidth: width, offsetHeight: height } = element;
    const above = anchor.above - height - GAP;

    setPosition({
      left: Math.max(
        VIEWPORT_MARGIN,
        Math.min(anchor.x - width / 2, window.innerWidth - width - VIEWPORT_MARGIN),
      ),
      top: above >= VIEWPORT_MARGIN ? above : anchor.below + GAP,
    });
  }, [anchor.above, anchor.below, anchor.x, range, total]);

  if (typeof document === 'undefined') return null;

  return createPortal(
    <div
      ref={ref}
      role="tooltip"
      data-histogram-readout
      className="pointer-events-none fixed z-[70] min-w-[168px] overflow-hidden rounded-md bg-[var(--floating-surface)] font-sans text-xs text-tx-0 shadow-popup"
      // Until it has been measured it has no place to stand.
      style={position ?? { left: 0, top: 0, visibility: 'hidden' }}
    >
      <div
        className={cn(
          'flex items-baseline justify-between gap-4 px-3 py-[6px] font-semibold tabular-nums',
          rows.length > 0 && 'border-b border-bd-0',
        )}
      >
        <span className="whitespace-nowrap">{range}</span>
        <span>{exact(total)}</span>
      </div>
      {rows.length > 0 && (
        <div className="px-3 py-[5px]">
          {rows.map((row) => (
            <div key={row.name} className="grid grid-cols-[8px_minmax(0,1fr)_auto] items-center gap-2 py-[2px]">
              <span className={cn('h-2 w-2 rounded-[2px]', row.fill)} />
              <span className="truncate text-tx-1">{row.name}</span>
              <span className="font-semibold tabular-nums">{exact(row.count)}</span>
            </div>
          ))}
        </div>
      )}
    </div>,
    document.body,
  );
}
