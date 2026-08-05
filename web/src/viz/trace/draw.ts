import type { Palette } from '@/viz/timeseries/themeAdapter';

import { colorKeyForService } from './colors';
import type { LaidOutTrace, Viewport } from './types';

export interface DrawConfig {
  ctx: CanvasRenderingContext2D;
  layout: LaidOutTrace;
  viewport: Viewport;
  /** CSS width / height in pixels (logical, not DPR-scaled). */
  width: number;
  height: number;
  rowHeight: number;
  palette: Palette;
  highlightSpanIds?: Set<string>;
  searchMatches?: Set<string>;
}

/**
 * Paint all visible spans. Spans narrower than 1 CSS pixel are skipped
 * outright; spans fully outside the viewport are culled by tree-agnostic
 * scan. Caller is responsible for clearing & DPR scaling the canvas before.
 */
export function drawSpans(cfg: DrawConfig): void {
  const { ctx, layout, viewport, width, height, rowHeight, palette, highlightSpanIds, searchMatches } = cfg;
  const visibleRowStart = Math.floor(viewport.scrollRow);
  const visibleRowEnd = Math.ceil(viewport.scrollRow + height / rowHeight);
  const range = viewport.toNs - viewport.fromNs;
  if (range <= 0) return;
  const xScale = width / range;

  for (const node of layout.nodes) {
    if (node.rowIndex < visibleRowStart || node.rowIndex > visibleRowEnd) continue;
    const x = (node.startOffsetNs - viewport.fromNs) * xScale;
    const w = node.durationNs * xScale;
    if (w < 1) continue; // sub-pixel cull
    if (x + w < 0 || x > width) continue;
    const y = (node.rowIndex - viewport.scrollRow) * rowHeight;

    const colorKey = colorKeyForService(node.span.service);
    const fill = palette[colorKey];
    ctx.fillStyle = fill;
    ctx.fillRect(x, y, Math.max(1, w - 1), rowHeight - 1);

    if (node.span.status === 'ERROR') {
      ctx.strokeStyle = palette['--red'];
      ctx.lineWidth = 1;
      ctx.strokeRect(x + 0.5, y + 0.5, Math.max(1, w - 2), rowHeight - 2);
    } else if (node.span.status === 'TIMED_OUT') {
      ctx.globalAlpha = 0.25;
      ctx.strokeStyle = palette['--yellow'];
      for (let dx = -rowHeight; dx < w; dx += 4) {
        ctx.beginPath();
        ctx.moveTo(x + dx, y);
        ctx.lineTo(x + dx + rowHeight, y + rowHeight);
        ctx.stroke();
      }
      ctx.globalAlpha = 1;
    }

    if (searchMatches && searchMatches.has(node.span.span_id)) {
      ctx.strokeStyle = palette['--accent'];
      ctx.lineWidth = 2;
      ctx.strokeRect(x + 1, y + 1, Math.max(1, w - 2), rowHeight - 2);
    }
    if (highlightSpanIds && highlightSpanIds.has(node.span.span_id)) {
      ctx.strokeStyle = palette['--fg'];
      ctx.lineWidth = 1.5;
      ctx.strokeRect(x + 0.5, y + 0.5, Math.max(1, w - 1), rowHeight - 1);
    }

    if (w > 30) {
      ctx.fillStyle = palette['--fg'];
      ctx.font = '11px Inter, ui-sans-serif, system-ui';
      ctx.textBaseline = 'middle';
      const label = `${node.span.service} · ${node.span.operation}`;
      const padded = label.length > Math.floor(w / 6) ? label.slice(0, Math.floor(w / 6)) + '…' : label;
      ctx.fillText(padded, x + 4, y + rowHeight / 2);
    }
  }
}

export function clearAndScale(canvas: HTMLCanvasElement, dpr: number, cssW: number, cssH: number) {
  canvas.width = Math.floor(cssW * dpr);
  canvas.height = Math.floor(cssH * dpr);
  canvas.style.width = `${cssW}px`;
  canvas.style.height = `${cssH}px`;
  const ctx = canvas.getContext('2d')!;
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  return ctx;
}
