import * as React from 'react';

import {
  INITIAL_VIEWPORT,
  MAX_ZOOM,
  type MapPoint,
  type MapSize,
  MIN_ZOOM,
  panBy,
  transformOf,
  type Viewport,
  ZOOM_STEP,
  zoomAbout,
} from './viewport';

/** How much a pixel of a mouse wheel zooms, and a pixel of a pinch on a trackpad (which arrives as a wheel with the control key). */
const WHEEL_ZOOM = 0.0015;
const PINCH_ZOOM = 0.01;
/** How much of the map a pressed arrow moves the view. */
const KEY_PAN = 0.12;

interface Pointer {
  x: number;
  y: number;
}

export interface MapViewportOptions {
  /**
   * What the wheel does: `always` zooms the map; `modifier` leaves the wheel to the page unless the
   * control (or command) key is held, as on a map set in the middle of a page that scrolls. A
   * pinch on a trackpad is a wheel with the control key, so it zooms either way.
   */
  wheel?: 'always' | 'modifier';
  /** Called when the wheel was left to the page for want of the key, so that the map can say how to zoom it. */
  onWheelHint?: () => void;
}

/**
 * The zoom and the position of a map, and what moves them: the buttons, the wheel and a pinch
 * (about the pointer, so the place under it stays under it), a double click, a drag, and the
 * keys. The wheel is the page's again where the map can zoom no further, so that scrolling on
 * past it scrolls the page.
 */
export function useMapViewport(size: MapSize, { wheel = 'always', onWheelHint }: MapViewportOptions = {}) {
  const svgRef = React.useRef<SVGSVGElement>(null);
  const [viewport, setViewport] = React.useState<Viewport>(INITIAL_VIEWPORT);
  const [dragging, setDragging] = React.useState(false);
  const viewportRef = React.useRef(viewport);
  const pointers = React.useRef(new Map<number, Pointer>());
  const pinch = React.useRef<{ distance: number; zoom: number } | null>(null);

  React.useEffect(() => {
    viewportRef.current = viewport;
  }, [viewport]);

  // The way the screen shows the map: where its origin is, and how many pixels a unit of it is.
  const matrix = React.useCallback(() => {
    const svg = svgRef.current;
    const screen = svg?.getScreenCTM?.();

    if (screen) return { scale: screen.a, left: screen.e, top: screen.f };

    const bounds = svg?.getBoundingClientRect();
    const scale = bounds && bounds.width > 0 ? bounds.width / size.width : 1;

    return { scale, left: bounds?.left ?? 0, top: bounds?.top ?? 0 };
  }, [size.width]);

  /** A point of the screen, in the units of the map as the reader sees it. */
  const toMap = React.useCallback(
    (clientX: number, clientY: number): MapPoint => {
      const { scale, left, top } = matrix();

      return { x: (clientX - left) / scale, y: (clientY - top) / scale };
    },
    [matrix],
  );

  const middle = React.useMemo<MapPoint>(() => ({ x: size.width / 2, y: size.height / 2 }), [size.height, size.width]);

  const zoomBy = React.useCallback(
    (factor: number, focus: MapPoint = middle) => setViewport((current) => zoomAbout(current, current.zoom * factor, focus, size)),
    [middle, size],
  );
  const zoomIn = React.useCallback(() => zoomBy(ZOOM_STEP), [zoomBy]);
  const zoomOut = React.useCallback(() => zoomBy(1 / ZOOM_STEP), [zoomBy]);
  const reset = React.useCallback(() => setViewport(INITIAL_VIEWPORT), []);

  // The wheel needs a listener that may stop the page from scrolling, which React's is not.
  React.useEffect(() => {
    const svg = svgRef.current;

    if (!svg) return undefined;

    const onWheel = (event: WheelEvent) => {
      const current = viewportRef.current.zoom;
      const lines = event.deltaMode === 1 ? 16 : event.deltaMode === 2 ? 400 : 1;
      const delta = event.deltaY * lines;

      if ((delta > 0 && current <= MIN_ZOOM) || (delta < 0 && current >= MAX_ZOOM) || delta === 0) return;

      if (wheel === 'modifier' && !event.ctrlKey && !event.metaKey) {
        onWheelHint?.();
        return;
      }

      event.preventDefault();

      const focus = toMap(event.clientX, event.clientY);
      const factor = Math.exp(-delta * (event.ctrlKey ? PINCH_ZOOM : WHEEL_ZOOM));

      setViewport((value) => zoomAbout(value, value.zoom * factor, focus, size));
    };

    svg.addEventListener('wheel', onWheel, { passive: false });

    return () => svg.removeEventListener('wheel', onWheel);
  }, [onWheelHint, size, toMap, wheel]);

  const onPointerDown = (event: React.PointerEvent<SVGSVGElement>) => {
    if (event.pointerType === 'mouse' && event.button !== 0) return;

    pointers.current.set(event.pointerId, { x: event.clientX, y: event.clientY });

    if (pointers.current.size === 2) {
      const [first, second] = [...pointers.current.values()] as [Pointer, Pointer];

      pinch.current = { distance: Math.hypot(first.x - second.x, first.y - second.y), zoom: viewportRef.current.zoom };
    }

    // A map that shows the whole of itself has nowhere to be dragged to.
    if (viewportRef.current.zoom > MIN_ZOOM || pointers.current.size === 2) {
      event.currentTarget.setPointerCapture(event.pointerId);
      setDragging(true);
    }
  };

  const onPointerMove = (event: React.PointerEvent<SVGSVGElement>) => {
    const before = pointers.current.get(event.pointerId);

    if (!before) return;

    pointers.current.set(event.pointerId, { x: event.clientX, y: event.clientY });

    const started = pinch.current;

    if (pointers.current.size >= 2 && started) {
      const [first, second] = [...pointers.current.values()] as [Pointer, Pointer];
      const distance = Math.hypot(first.x - second.x, first.y - second.y);
      const focus = toMap((first.x + second.x) / 2, (first.y + second.y) / 2);

      setViewport((current) => zoomAbout(current, started.zoom * (distance / Math.max(1, started.distance)), focus, size));
      return;
    }

    if (viewportRef.current.zoom <= MIN_ZOOM) return;

    const { scale } = matrix();

    setViewport((current) => panBy(current, (event.clientX - before.x) / scale, (event.clientY - before.y) / scale, size));
  };

  const onPointerEnd = (event: React.PointerEvent<SVGSVGElement>) => {
    if (!pointers.current.delete(event.pointerId)) return;

    if (event.currentTarget.hasPointerCapture?.(event.pointerId)) event.currentTarget.releasePointerCapture(event.pointerId);
    if (pointers.current.size < 2) pinch.current = null;
    if (pointers.current.size === 0) setDragging(false);
  };

  const onDoubleClick = (event: React.MouseEvent<SVGSVGElement>) => {
    event.preventDefault();
    zoomBy(ZOOM_STEP, toMap(event.clientX, event.clientY));
  };

  /** `+` and `-` zoom, `0` shows the whole map, and the arrows move a zoomed one. */
  const onKeyDown = (event: React.KeyboardEvent) => {
    if (event.ctrlKey || event.metaKey || event.altKey) return;

    const moves: Record<string, [number, number]> = {
      ArrowLeft: [KEY_PAN, 0],
      ArrowRight: [-KEY_PAN, 0],
      ArrowUp: [0, KEY_PAN],
      ArrowDown: [0, -KEY_PAN],
    };
    const move = moves[event.key];

    if (move) {
      // The arrows scroll the page while the map shows all of itself.
      if (viewportRef.current.zoom <= MIN_ZOOM) return;

      event.preventDefault();
      setViewport((current) => panBy(current, move[0] * size.width, move[1] * size.height, size));
      return;
    }

    if (event.key === '+' || event.key === '=') zoomIn();
    else if (event.key === '-' || event.key === '_') zoomOut();
    else if (event.key === '0') reset();
    else return;

    event.preventDefault();
  };

  return {
    svgRef,
    viewport,
    dragging,
    transform: transformOf(viewport, size),
    canZoomIn: viewport.zoom < MAX_ZOOM,
    canZoomOut: viewport.zoom > MIN_ZOOM,
    zoomIn,
    zoomOut,
    reset,
    onKeyDown,
    svgHandlers: {
      onPointerDown,
      onPointerMove,
      onPointerUp: onPointerEnd,
      onPointerCancel: onPointerEnd,
      onDoubleClick,
    },
  };
}
