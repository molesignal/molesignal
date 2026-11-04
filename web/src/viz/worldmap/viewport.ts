/** How the map is shown: how far in, and where the middle of the view is. Pan is in the units of the map. */
export interface Viewport {
  zoom: number;
  panX: number;
  panY: number;
}

export interface MapSize {
  width: number;
  height: number;
}

export interface MapPoint {
  x: number;
  y: number;
}

export const MIN_ZOOM = 1;
export const MAX_ZOOM = 8;
/** What a press of a zoom button, or a double click, multiplies the zoom by. */
export const ZOOM_STEP = 1.5;

export const INITIAL_VIEWPORT: Viewport = { zoom: MIN_ZOOM, panX: 0, panY: 0 };

export const clamp = (value: number, minimum: number, maximum: number): number => Math.min(maximum, Math.max(minimum, value));

/** The viewport as it can be: zoomed no further than the limits, and panned no further than the map reaches. */
export function clampViewport(viewport: Viewport, size: MapSize): Viewport {
  const zoom = clamp(viewport.zoom, MIN_ZOOM, MAX_ZOOM);

  if (zoom === MIN_ZOOM) return INITIAL_VIEWPORT;

  const reachX = (size.width * (zoom - 1)) / 2;
  const reachY = (size.height * (zoom - 1)) / 2;

  return { zoom, panX: clamp(viewport.panX, -reachX, reachX), panY: clamp(viewport.panY, -reachY, reachY) };
}

/**
 * The viewport after zooming to `zoom` about `focus`, a point in the units of the map as the
 * reader sees it: the part of the map under it stays under it, as under a pointer that zooms.
 */
export function zoomAbout(viewport: Viewport, zoom: number, focus: MapPoint, size: MapSize): Viewport {
  const next = clamp(zoom, MIN_ZOOM, MAX_ZOOM);
  const centreX = size.width / 2;
  const centreY = size.height / 2;
  // Where the focus is on the map itself, before the zoom.
  const mapX = (focus.x - viewport.panX - centreX) / viewport.zoom + centreX;
  const mapY = (focus.y - viewport.panY - centreY) / viewport.zoom + centreY;

  return clampViewport(
    { zoom: next, panX: focus.x - centreX - next * (mapX - centreX), panY: focus.y - centreY - next * (mapY - centreY) },
    size,
  );
}

/** The viewport moved by `dx` and `dy`, in the units of the map, as far as the map reaches. */
export function panBy(viewport: Viewport, dx: number, dy: number, size: MapSize): Viewport {
  return clampViewport({ ...viewport, panX: viewport.panX + dx, panY: viewport.panY + dy }, size);
}

/** The transform of the layers of the map, about the middle of it. */
export function transformOf(viewport: Viewport, size: MapSize): string {
  return `translate(${viewport.panX} ${viewport.panY}) translate(${size.width / 2} ${size.height / 2}) scale(${viewport.zoom}) translate(${-size.width / 2} ${-size.height / 2})`;
}
