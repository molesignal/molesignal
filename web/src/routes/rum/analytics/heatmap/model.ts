import type { ClickedElement, RumFilter, ScrollBand } from '@/api/rum';

/** How many cells an element is cut into each way, as the server counts them. */
export const CELLS = 10;

/** The sizes of window a map can be of: a page is laid out differently in each. */
export const DEVICES = ['desktop', 'tablet', 'mobile', 'all'] as const;

export type Device = (typeof DEVICES)[number];

export const parseDevice = (value: string | null): Device => DEVICES.find((device) => device === value) ?? 'desktop';

/** The two maps of a page. */
export type MapKind = 'clicks' | 'scroll';

export const parseKind = (value: string | null): MapKind => (value === 'scroll' ? 'scroll' : 'clicks');

/** The window a size means, in CSS pixels of its width. */
export function deviceFilters(device: Device): RumFilter[] {
  const wide = (op: '>=' | '<', width: number): RumFilter => ({ field: 'display.viewport.width', op, value: String(width) });

  switch (device) {
    case 'desktop':
      return [wide('>=', 1024)];
    case 'tablet':
      return [wide('>=', 600), wide('<', 1024)];
    case 'mobile':
      return [wide('<', 600)];
    case 'all':
      return [];
  }
}

/** A rectangle of the page, in CSS pixels. */
export interface Box {
  left: number;
  top: number;
  width: number;
  height: number;
}

/** A spot of heat on the page, in CSS pixels of the page. */
export interface Spot {
  x: number;
  y: number;
  clicks: number;
  element: string;
}

/**
 * Where the clicks of the elements fall on a page. An element found in the recording takes its clicks
 * cell by cell where it stands now; the clicks that do not say where in the element they fell go to its
 * middle. An element the recording does not have is placed where on the page its clicks landed, and is
 * said to be off it when they do not say that either.
 */
export function spotsOf(
  elements: readonly ClickedElement[],
  locate: (element: string) => Box | null,
): { spots: Spot[]; off: Set<string> } {
  const spots: Spot[] = [];
  const off = new Set<string>();

  for (const element of elements) {
    const box = locate(element.element);

    if (box) {
      const placed = element.cells.reduce((total, [, , clicks]) => total + clicks, 0);

      for (const [x, y, clicks] of element.cells) {
        spots.push({
          x: box.left + ((x + 0.5) / CELLS) * box.width,
          y: box.top + ((y + 0.5) / CELLS) * box.height,
          clicks,
          element: element.element,
        });
      }

      if (element.clicks > placed) {
        spots.push({
          x: box.left + box.width / 2,
          y: box.top + box.height / 2,
          clicks: element.clicks - placed,
          element: element.element,
        });
      }
    } else if (typeof element.pageX === 'number' && typeof element.pageY === 'number') {
      spots.push({ x: element.pageX, y: element.pageY, clicks: element.clicks, element: element.element });
      off.add(element.element);
    } else {
      off.add(element.element);
    }
  }

  return { spots, off };
}

/** The colour of a share of heat, from 0 to 1: cool and see-through to hot and solid, as `[r, g, b, a]`. */
export function heatColor(share: number): [number, number, number, number] {
  const value = Math.min(1, Math.max(0, share));
  const stops: Array<[number, number, number, number]> = [
    [0.0, 37, 99, 235],
    [0.35, 22, 163, 74],
    [0.65, 234, 179, 8],
    [1.0, 220, 38, 38],
  ];

  let lower = stops[0] as [number, number, number, number];
  let upper = stops[stops.length - 1] as [number, number, number, number];

  for (let index = 0; index + 1 < stops.length; index += 1) {
    const [from, to] = [stops[index], stops[index + 1]] as [typeof lower, typeof upper];

    if (value >= from[0] && value <= to[0]) {
      lower = from;
      upper = to;
      break;
    }
  }

  const span = upper[0] - lower[0] || 1;
  const along = (value - lower[0]) / span;
  const mix = (channel: 1 | 2 | 3) => Math.round(lower[channel] + (upper[channel] - lower[channel]) * along);

  return [mix(1), mix(2), mix(3), Math.round(70 + 150 * value)];
}

/** How far down a share of the views got: the band a share first falls under. */
export function depthReached(bands: readonly ScrollBand[], share: number): number | null {
  let depth: number | null = null;

  for (const band of bands) {
    if (band.reached >= share) depth = band.depth;
    else break;
  }

  return depth;
}

/** The share of views that got to a depth, as the scroll map tells it. */
export function reachedAt(bands: readonly ScrollBand[], depth: number): number {
  let share = 0;

  for (const band of bands) {
    if (band.depth > depth) break;
    share = band.reached;
  }

  return share;
}
