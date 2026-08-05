/**
 * AnchorRenderer — shared visual marker for the pinned investigation anchor.
 *
 * Reads `anchor` from the global time store and renders:
 *   - a 1px `--accent` vertical line at the supplied x position
 *   - a `PIN hh:mm:ss` UTC badge anchored to the top of the chart container
 *
 * Visualizations pass `xForTimestamp(at)` so the renderer is layout-agnostic.
 * If no anchor is pinned, the component renders nothing.
 *
 * Spec: web-investigation-shell.
 */
import * as React from 'react';

import { useTimeStore } from '@/stores/useTimeStore';

export interface AnchorRendererProps {
  /** Pixel x coordinate of the anchor inside the chart. Caller computes via `plot.valToPos`. */
  xForTimestamp: (atMs: number) => number | null;
  /** Container height (px). Used for the vertical line stretch. */
  height: number;
  /** Optional className for the absolute wrapper. */
  className?: string;
}

export const AnchorRenderer: React.FC<AnchorRendererProps> = ({
  xForTimestamp,
  height,
  className,
}) => {
  const anchor = useTimeStore((s) => s.anchor);
  if (!anchor) return null;

  const atMs = Date.parse(anchor.at);
  if (!Number.isFinite(atMs)) return null;

  const x = xForTimestamp(atMs);
  if (x == null) return null;

  return (
    <div
      aria-hidden="true"
      className={className}
      data-testid="anchor-renderer"
      style={{
        position: 'absolute',
        top: 0,
        left: 0,
        pointerEvents: 'none',
        width: '100%',
        height,
      }}
    >
      <div
        style={{
          position: 'absolute',
          top: 0,
          left: x,
          width: 1,
          height,
          background: 'var(--accent)',
        }}
      />
      <div
        style={{
          position: 'absolute',
          top: 0,
          left: x,
          transform: 'translateX(-50%)',
          padding: '2px 6px',
          background: 'var(--surface)',
          color: 'var(--fg)',
          border: '1px solid var(--accent)',
          borderRadius: 3,
          fontFamily: 'var(--font-sans)',
          fontSize: 11,
          lineHeight: 1.2,
          whiteSpace: 'nowrap',
        }}
      >
        PIN {formatHhMmSsUtc(atMs)}
      </div>
    </div>
  );
};

export function formatHhMmSsUtc(atMs: number): string {
  const d = new Date(atMs);
  const hh = String(d.getUTCHours()).padStart(2, '0');
  const mm = String(d.getUTCMinutes()).padStart(2, '0');
  const ss = String(d.getUTCSeconds()).padStart(2, '0');
  return `${hh}:${mm}:${ss}`;
}
