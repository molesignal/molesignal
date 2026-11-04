import * as React from 'react';

import { formatTraceDurationNs } from '../duration';
import { percentAcross, timeTicks } from '../model/axis';

/** The width a node takes, measured and kept up to date. */
export function useMeasuredWidth<T extends HTMLElement>(): [React.RefObject<T>, number] {
  const ref = React.useRef<T>(null);
  const [width, setWidth] = React.useState(0);

  React.useEffect(() => {
    const element = ref.current;
    if (!element) return undefined;

    const measure = () => setWidth(element.getBoundingClientRect().width);
    measure();
    if (typeof ResizeObserver === 'undefined') return undefined;

    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  return [ref, width];
}

/** Where the marks stand for the stretch in view, and the width they were laid out for. */
export function useTimeTicks(fromNs: number, toNs: number, widthPx: number): number[] {
  return React.useMemo(() => timeTicks(fromNs, toNs, widthPx || 600), [fromNs, toNs, widthPx]);
}

/**
 * The time axis over the waterfall: round lengths of time from the start of
 * the trace, so a span's start and length can be counted off it.
 */
export function TimeAxis({ ticks, fromNs, toNs }: { ticks: readonly number[]; fromNs: number; toNs: number }) {
  return (
    <div className="relative h-full" data-testid="trace-time-axis">
      {ticks.map((tick, index) => {
        const left = percentAcross(tick, fromNs, toNs);
        const last = index === ticks.length - 1 && left > 92;

        return (
          <span
            key={tick}
            className="absolute top-1/2 whitespace-nowrap font-sans text-xs tabular-nums text-tx-3"
            style={{ left: `${left}%`, transform: `translate(${index === 0 ? '4px' : last ? 'calc(-100% - 4px)' : '-50%'}, -50%)` }}
          >
            {tick === 0 ? '0' : formatTraceDurationNs(tick)}
          </span>
        );
      })}
    </div>
  );
}

/** The marks of the axis carried down through the lines, faint enough to read past. */
export function TimeGrid({ ticks, fromNs, toNs }: { ticks: readonly number[]; fromNs: number; toNs: number }) {
  return (
    <div aria-hidden="true" className="pointer-events-none absolute inset-0">
      {ticks.map((tick) => (
        <span key={tick} className="absolute inset-y-0 w-px bg-bd-0" style={{ left: `${percentAcross(tick, fromNs, toNs)}%` }} />
      ))}
    </div>
  );
}
