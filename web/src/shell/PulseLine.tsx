import * as React from 'react';

import { cn } from '@/shell/lib/cn';

/**
 * The product mark's pulse as a graphic that can say three things:
 *
 *  - `empty`   a flat line with one beat that draws in, then a dotted tail where
 *              the signal is not coming from yet. "Nothing here yet."
 *  - `travel`  the same line with a bright beat running along it, on a loop.
 *              "Working on it."
 *  - `mark`    the whole line, drawn once, in the signal gradient.
 *
 * It is decorative: whatever it illustrates is named by the text beside it.
 * Colour comes from `currentColor` for the line and from the shared signal
 * gradient for the beat.
 */
export type PulseLineVariant = 'empty' | 'travel' | 'mark';

// Viewbox 64x24. The beat is the logo's QRS shape, scaled to this box.
const BASELINE_LEFT = 'M2 12H22';
const BEAT = 'M22 12L25 14.5L30 2.5L37 21.5L41 9L43.5 12';
const BASELINE_RIGHT = 'M43.5 12H62';
const WHOLE = 'M2 12H22L25 14.5L30 2.5L37 21.5L41 9L43.5 12H62';

export function PulseLine({
  variant = 'empty',
  className,
}: {
  variant?: PulseLineVariant | undefined;
  className?: string | undefined;
}) {
  const gradientId = React.useId();
  const stroke = `url(#${gradientId})`;
  const common = {
    fill: 'none',
    strokeWidth: 1.75,
    strokeLinecap: 'round',
    strokeLinejoin: 'round',
  } as const;

  return (
    <svg
      aria-hidden="true"
      focusable="false"
      viewBox="0 0 64 24"
      className={cn('shrink-0 overflow-visible', className)}
    >
      <defs>
        <linearGradient id={gradientId} x1="0" y1="0" x2="64" y2="0" gradientUnits="userSpaceOnUse">
          <stop offset="0%" stopColor="var(--indigo)" />
          <stop offset="55%" stopColor="var(--blue)" />
          <stop offset="100%" stopColor="var(--green)" />
        </linearGradient>
      </defs>

      {variant === 'empty' && (
        <>
          <path d={BASELINE_LEFT} stroke="currentColor" {...common} />
          <path
            d={BASELINE_RIGHT}
            stroke="currentColor"
            {...common}
            strokeDasharray="0.5 5"
            opacity={0.6}
          />
          <path d={BEAT} pathLength={1} className="ms-pulse-draw" stroke={stroke} {...common} />
        </>
      )}

      {variant === 'travel' && (
        <>
          <path d={WHOLE} stroke="currentColor" {...common} opacity={0.22} />
          <path d={WHOLE} pathLength={1} className="ms-pulse-travel" stroke={stroke} {...common} />
        </>
      )}

      {variant === 'mark' && (
        <path d={WHOLE} pathLength={1} className="ms-pulse-draw" stroke={stroke} {...common} />
      )}
    </svg>
  );
}
