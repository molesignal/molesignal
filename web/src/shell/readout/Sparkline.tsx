import * as React from 'react';

import { cn } from '@/shell/lib/cn';

import {
  SPARKLINE_HEIGHT,
  SPARKLINE_WIDTH,
  sparklineBars,
  sparklineGeometry,
} from './trendGeometry';

/**
 * A trend at a glance: the shape of a figure over the window, with no axes.
 *
 * It says "rising, falling, spiky", not how much: the figure beside it carries
 * the number. `line` is for levels (latency, a rate), `bars` for counts per
 * bucket. Colour comes from `currentColor`, so the caller sets the tone with a
 * text colour. Decorative: the reading it accompanies is the accessible one.
 */
export function Sparkline({
  values,
  variant = 'line',
  zeroBased = false,
  className,
}: {
  values: ReadonlyArray<number | null | undefined>;
  variant?: 'line' | 'bars';
  /** Scale from zero, for figures where "how far from nothing" is the point. */
  zeroBased?: boolean;
  className?: string | undefined;
}) {
  const gradientId = React.useId();
  const geometry = variant === 'line' ? sparklineGeometry(values, { zeroBased }) : null;
  const bars = variant === 'bars' ? sparklineBars(values) : [];
  const empty = variant === 'line' ? !geometry : bars.every((bar) => bar.height === 0);

  return (
    <div
      aria-hidden="true"
      data-sparkline={empty ? 'empty' : variant}
      className={cn('relative h-[32px] w-full', className)}
    >
      {empty ? (
        // Nothing to draw is still a place in the row: a quiet baseline holds it.
        <div className="absolute inset-x-0 bottom-0 h-px bg-bd-1" />
      ) : (
        <svg
          viewBox={`0 0 ${SPARKLINE_WIDTH} ${SPARKLINE_HEIGHT}`}
          preserveAspectRatio="none"
          className="ms-wipe-in block h-full w-full overflow-visible"
        >
          {geometry && (
            <>
              <defs>
                <linearGradient id={gradientId} x1="0" y1="0" x2="0" y2="1">
                  <stop offset="0%" stopColor="currentColor" stopOpacity="0.2" />
                  <stop offset="100%" stopColor="currentColor" stopOpacity="0" />
                </linearGradient>
              </defs>
              <path d={geometry.area} fill={`url(#${gradientId})`} />
              <path
                d={geometry.line}
                fill="none"
                stroke="currentColor"
                strokeWidth="1.5"
                strokeLinejoin="round"
                strokeLinecap="round"
                vectorEffect="non-scaling-stroke"
              />
            </>
          )}
          {bars.map((bar, index) => (
            <rect
              key={index}
              x={bar.x}
              y={bar.y}
              width={bar.width}
              height={bar.height}
              rx="0.6"
              fill="currentColor"
              opacity={0.5 + 0.5 * ((index + 1) / bars.length)}
            />
          ))}
        </svg>
      )}
      {geometry && (
        // The newest sample. A circle in the stretched SVG would be an ellipse.
        <span
          className="ms-wipe-dot absolute h-[5px] w-[5px] -translate-x-1/2 -translate-y-1/2 rounded-full bg-current [box-shadow:0_0_0_2px_var(--functional-surface)]"
          style={{ left: `${geometry.end.x}%`, top: `${geometry.end.y}%` }}
        />
      )}
    </div>
  );
}
