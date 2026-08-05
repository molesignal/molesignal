import * as React from 'react';

/**
 * MoleSignal brand mark — an ECG waveform stroked with a horizontal
 * indigo → blue → green gradient. The three stops echo the data viz
 * series (chart-1 indigo / chart-7 sky blue / chart-4 green) so the logo
 * reads as "the same product that shows you the three signals."
 *
 * Phase 4 retired the legacy red→orange→blue→green four-stop gradient
 * (the "terminal-hacker" warmth violated the Confident-quiet brief).
 */
export function LogoMark({ size = 22, className }: { size?: number; className?: string }) {
  const id = React.useId();
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 32 32"
      fill="none"
      className={className}
      aria-hidden
    >
      <defs>
        <linearGradient id={`ecg-${id}`} x1="0" y1="16" x2="32" y2="16" gradientUnits="userSpaceOnUse">
          <stop offset="0%" stopColor="var(--indigo)" />
          <stop offset="50%" stopColor="var(--blue)" />
          <stop offset="100%" stopColor="var(--green)" />
        </linearGradient>
      </defs>
      <path
        d="M0 16 H8 L10 17.5 L13 4 L18 28 L21.5 13 L23 16 H32"
        stroke={`url(#ecg-${id})`}
        strokeWidth="2.6"
        strokeLinecap="round"
        strokeLinejoin="round"
        fill="none"
      />
    </svg>
  );
}
