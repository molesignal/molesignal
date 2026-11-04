import { describe, expect, it } from 'vitest';

import {
  clampViewport,
  INITIAL_VIEWPORT,
  MAX_ZOOM,
  type MapPoint,
  MIN_ZOOM,
  panBy,
  transformOf,
  type Viewport,
  zoomAbout,
} from './viewport';

const SIZE = { width: 1_000, height: 500 };

/** Where a point of the map is shown, with the viewport as it is. */
const shown = (viewport: Viewport, point: MapPoint): MapPoint => ({
  x: viewport.panX + SIZE.width / 2 + viewport.zoom * (point.x - SIZE.width / 2),
  y: viewport.panY + SIZE.height / 2 + viewport.zoom * (point.y - SIZE.height / 2),
});

describe('the viewport of the map', () => {
  it('opens on the whole map', () => {
    expect(INITIAL_VIEWPORT).toEqual({ zoom: 1, panX: 0, panY: 0 });
    expect(transformOf(INITIAL_VIEWPORT, SIZE)).toBe('translate(0 0) translate(500 250) scale(1) translate(-500 -250)');
  });

  it('zooms no further than its limits, and a map zoomed all the way out is not moved', () => {
    expect(zoomAbout(INITIAL_VIEWPORT, 100, { x: 500, y: 250 }, SIZE).zoom).toBe(MAX_ZOOM);
    expect(zoomAbout({ zoom: 2, panX: 100, panY: 40 }, 0.2, { x: 10, y: 10 }, SIZE)).toEqual(INITIAL_VIEWPORT);
    expect(clampViewport({ zoom: MIN_ZOOM, panX: 50, panY: 50 }, SIZE)).toEqual(INITIAL_VIEWPORT);
  });

  it('zooms about a point, which stays where it is under the pointer', () => {
    const focus = { x: 700, y: 180 };
    // The point of the map under the pointer before the zoom.
    const before = { x: 700, y: 180 };
    const zoomed = zoomAbout(INITIAL_VIEWPORT, 3, focus, SIZE);

    expect(zoomed.zoom).toBe(3);
    expect(shown(zoomed, before).x).toBeCloseTo(focus.x, 6);
    expect(shown(zoomed, before).y).toBeCloseTo(focus.y, 6);

    // And again, from a viewport that is not at the middle.
    const further = zoomAbout(zoomed, 5, { x: 300, y: 300 }, SIZE);
    const under = { x: (300 - zoomed.panX - 500) / zoomed.zoom + 500, y: (300 - zoomed.panY - 250) / zoomed.zoom + 250 };

    expect(shown(further, under).x).toBeCloseTo(300, 6);
    expect(shown(further, under).y).toBeCloseTo(300, 6);
  });

  it('does not pan past the edge of the map', () => {
    const edge = panBy({ zoom: 2, panX: 0, panY: 0 }, 10_000, -10_000, SIZE);

    expect(edge).toEqual({ zoom: 2, panX: 500, panY: -250 });
    // At the edge the map still covers the view: its left edge is at the view's.
    expect(shown(edge, { x: 0, y: 0 }).x).toBeCloseTo(0, 6);
    expect(shown(edge, { x: SIZE.width, y: SIZE.height }).y).toBeCloseTo(SIZE.height, 6);
  });

  it('pans by what it is moved by, within the edges', () => {
    expect(panBy({ zoom: 4, panX: 0, panY: 0 }, 30, -20, SIZE)).toEqual({ zoom: 4, panX: 30, panY: -20 });
    expect(panBy(INITIAL_VIEWPORT, 30, -20, SIZE)).toEqual(INITIAL_VIEWPORT);
  });
});
