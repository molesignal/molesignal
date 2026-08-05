import uPlot from 'uplot';

export interface TimeRange {
  from: number;
  to: number;
}

export interface TimeRangeInteractionState extends TimeRange {
  isTimeRangePending: boolean;
}

export interface BrushHandlers {
  /** Emitted after a plot selection or double-click zoom-out. */
  onRangeSelect?: (range: TimeRange) => void;
  /** X-axis pan callback; receives a delta in seconds. */
  onPan?: (deltaSecs: number) => void;
  /** Keeps an in-progress X-axis pan stable while its query range catches up. */
  onRangeInteractionChange?: (
    state: TimeRangeInteractionState | null,
  ) => void;
  onClick?: () => void;
  /** Legacy fallback for consumers that do not provide onRangeSelect. */
  onRangeReset?: () => void;
}

const MIN_RANGE_DRAG_PX = 5;
const RANGE_MATCH_TOLERANCE_SECONDS = 0.001;

/**
 * Installs the same time-range interactions used by Grafana's current uPlot
 * time-series panel:
 *
 * - native uPlot selection in the plot area, committed on mouseup;
 * - live X-axis panning with document-level mouse tracking;
 * - double-click zoom-out to twice the current duration.
 *
 * Wheel input is intentionally left to the browser. Grafana does not bind a
 * wheel zoom handler to its time-series panel.
 */
export function installBrush(plot: uPlot, handlers: BrushHandlers): () => void {
  const over = plot.over as HTMLElement;
  const selectEl = plot.root.querySelector<HTMLElement>('.u-select');
  const originalSelectBackground = selectEl?.style.background ?? '';
  let xAxisEl: HTMLElement | null = null;
  let activeMoveListener: ((event: MouseEvent) => void) | null = null;
  let activeUpListener: ((event: MouseEvent) => void) | null = null;

  const readVisibleRange = (): TimeRange | null => {
    const min = plot.scales.x?.min;
    const max = plot.scales.x?.max;
    if (
      typeof min !== 'number' ||
      typeof max !== 'number' ||
      !Number.isFinite(min) ||
      !Number.isFinite(max) ||
      max <= min
    ) {
      return null;
    }
    return { from: min, to: max };
  };

  const syncVisibleRangeData = (): void => {
    const range = readVisibleRange();
    if (!range) return;
    over.dataset.visibleFrom = String(range.from);
    over.dataset.visibleTo = String(range.to);
  };

  const applyVisibleRange = (range: TimeRange): void => {
    plot.setScale('x', { min: range.from, max: range.to });
    syncVisibleRangeData();
  };

  const onSetScale = (_instance: uPlot, scaleKey: string): void => {
    if (scaleKey === 'x') syncVisibleRangeData();
  };

  const onSetSelect = (instance: uPlot): void => {
    const event = instance.cursor.event;
    const isZoomAction = Boolean(event && !event.ctrlKey && !event.metaKey);
    if (
      isZoomAction &&
      handlers.onRangeSelect &&
      instance.select.width >= MIN_RANGE_DRAG_PX
    ) {
      const left = instance.select.left;
      const right = left + instance.select.width;
      handlers.onRangeSelect({
        from: instance.posToVal(left, 'x'),
        to: instance.posToVal(right, 'x'),
      });
    }

    // cursor.drag.setScale is false, so uPlot leaves the selection visible.
    instance.setSelect({ left: 0, width: 0, top: 0, height: 0 }, false);
  };

  const setScaleHooks = plot.hooks.setScale ?? [];
  const setSelectHooks = plot.hooks.setSelect ?? [];
  plot.hooks.setScale = setScaleHooks;
  plot.hooks.setSelect = setSelectHooks;
  setScaleHooks.push(onSetScale);
  setSelectHooks.push(onSetSelect);
  syncVisibleRangeData();

  if (selectEl) {
    selectEl.dataset.testid = 'chart-range-selection';
    selectEl.style.background = 'rgba(120, 120, 130, 0.2)';
  }

  const isZoomAction = (event: MouseEvent): boolean =>
    event.button === 0 && !event.ctrlKey && !event.metaKey;

  const onPlotMouseDown = (event: MouseEvent): void => {
    if (!handlers.onRangeSelect || !isZoomAction(event)) return;
    over.classList.add('zoom-drag');

    const onMouseUp = (): void => {
      over.classList.remove('zoom-drag');
      document.removeEventListener('mouseup', onMouseUp, true);
    };
    document.addEventListener('mouseup', onMouseUp, true);
  };

  const onPlotClick = (event: MouseEvent): void => {
    if (
      event.target === over &&
      !event.ctrlKey &&
      !event.metaKey
    ) {
      handlers.onClick?.();
    }
  };

  const onDoubleClick = (event: MouseEvent): void => {
    if (event.ctrlKey || event.metaKey) return;
    const current = readVisibleRange();
    if (current && handlers.onRangeSelect) {
      const pad = (current.to - current.from) / 2;
      event.preventDefault();
      handlers.onRangeSelect({
        from: current.from - pad,
        to: current.to + pad,
      });
      return;
    }
    if (handlers.onRangeReset) {
      event.preventDefault();
      handlers.onRangeReset();
    }
  };

  over.addEventListener('mousedown', onPlotMouseDown, true);
  over.addEventListener('click', onPlotClick);
  over.addEventListener('dblclick', onDoubleClick);

  const axis = (
    plot.axes.find((candidate) => candidate.scale === 'x') as
      | (uPlot.Axis & { _el?: HTMLElement })
      | undefined
  )?._el;
  if (
    axis instanceof HTMLElement &&
    (handlers.onRangeSelect || handlers.onPan)
  ) {
    xAxisEl = axis;
    xAxisEl.dataset.rangePan = 'horizontal-drag';

    const onAxisMouseEnter = (): void => {
      if (xAxisEl) xAxisEl.style.cursor = 'grab';
    };
    const onAxisMouseLeave = (): void => {
      if (xAxisEl) xAxisEl.style.cursor = '';
    };
    const onAxisMouseDown = (event: MouseEvent): void => {
      if (event.button !== 0) return;
      const startRange = readVisibleRange();
      if (!startRange || plot.bbox.width <= 0) return;

      event.preventDefault();
      xAxisEl!.style.cursor = 'grabbing';

      const rect = over.getBoundingClientRect();
      const startX = event.clientX - rect.left;

      const onMove = (moveEvent: MouseEvent): void => {
        moveEvent.preventDefault();
        const currentX = moveEvent.clientX - rect.left;
        const next = calculatePanRange(
          startRange.from,
          startRange.to,
          currentX - startX,
          plot.bbox.width,
        );
        handlers.onRangeInteractionChange?.({
          ...next,
          isTimeRangePending: false,
        });
        applyVisibleRange(next);
      };

      const onUp = (upEvent: MouseEvent): void => {
        const endX = upEvent.clientX - rect.left;
        const dragPixels = endX - startX;
        xAxisEl!.style.cursor = 'grab';

        if (Math.abs(dragPixels) >= MIN_RANGE_DRAG_PX) {
          const next = calculatePanRange(
            startRange.from,
            startRange.to,
            dragPixels,
            plot.bbox.width,
          );
          handlers.onRangeInteractionChange?.({
            ...next,
            isTimeRangePending: true,
          });
          if (handlers.onPan) {
            handlers.onPan(next.from - startRange.from);
          } else {
            handlers.onRangeSelect?.(next);
          }
        } else {
          handlers.onRangeInteractionChange?.(null);
          applyVisibleRange(startRange);
        }

        document.removeEventListener('mousemove', onMove);
        document.removeEventListener('mouseup', onUp);
        activeMoveListener = null;
        activeUpListener = null;
      };

      if (activeMoveListener) {
        document.removeEventListener('mousemove', activeMoveListener);
      }
      if (activeUpListener) {
        document.removeEventListener('mouseup', activeUpListener);
      }
      activeMoveListener = onMove;
      activeUpListener = onUp;
      document.addEventListener('mousemove', onMove);
      document.addEventListener('mouseup', onUp);
    };

    xAxisEl.addEventListener('mouseenter', onAxisMouseEnter);
    xAxisEl.addEventListener('mouseleave', onAxisMouseLeave);
    xAxisEl.addEventListener('mousedown', onAxisMouseDown);

    return () => {
      over.removeEventListener('mousedown', onPlotMouseDown, true);
      over.removeEventListener('click', onPlotClick);
      over.removeEventListener('dblclick', onDoubleClick);
      removeHook(setScaleHooks, onSetScale);
      removeHook(setSelectHooks, onSetSelect);
      xAxisEl?.removeEventListener('mouseenter', onAxisMouseEnter);
      xAxisEl?.removeEventListener('mouseleave', onAxisMouseLeave);
      xAxisEl?.removeEventListener('mousedown', onAxisMouseDown);
      if (xAxisEl) {
        xAxisEl.style.cursor = '';
        delete xAxisEl.dataset.rangePan;
      }
      if (activeMoveListener) {
        document.removeEventListener('mousemove', activeMoveListener);
      }
      if (activeUpListener) {
        document.removeEventListener('mouseup', activeUpListener);
      }
      cleanupInteractionAttributes(over, selectEl, originalSelectBackground);
    };
  }

  return () => {
    over.removeEventListener('mousedown', onPlotMouseDown, true);
    over.removeEventListener('click', onPlotClick);
    over.removeEventListener('dblclick', onDoubleClick);
    removeHook(setScaleHooks, onSetScale);
    removeHook(setSelectHooks, onSetSelect);
    cleanupInteractionAttributes(over, selectEl, originalSelectBackground);
  };
}

/**
 * Grafana calculates with uPlot's device-pixel bbox, then divides it by the
 * current pixel ratio to obtain CSS pixels.
 */
export function calculatePanRange(
  timeFrom: number,
  timeTo: number,
  dragPixels: number,
  plotWidth: number,
): TimeRange {
  if (!Number.isFinite(plotWidth) || plotWidth <= 0) {
    return { from: timeFrom, to: timeTo };
  }
  const unitsPerPixel = (timeTo - timeFrom) / (plotWidth / uPlot.pxRatio);
  const timeShift = dragPixels * unitsPerPixel;
  return {
    from: timeFrom - timeShift,
    to: timeTo - timeShift,
  };
}

/**
 * Holds Grafana-style live pan bounds until the external query time range
 * catches up. This prevents old data from snapping the chart backwards after
 * mouseup.
 */
export function resolveInteractionRange(
  externalRange: readonly [number, number],
  interaction: TimeRangeInteractionState | null,
): {
  range: [number, number];
  interaction: TimeRangeInteractionState | null;
} {
  if (!interaction) {
    return {
      range: [externalRange[0], externalRange[1]],
      interaction: null,
    };
  }

  if (interaction.isTimeRangePending) {
    const externalMatches =
      Math.abs(externalRange[0] - interaction.from) <=
        RANGE_MATCH_TOLERANCE_SECONDS &&
      Math.abs(externalRange[1] - interaction.to) <=
        RANGE_MATCH_TOLERANCE_SECONDS;
    if (externalMatches) {
      return {
        range: [externalRange[0], externalRange[1]],
        interaction: null,
      };
    }
  }

  return {
    range: [interaction.from, interaction.to],
    interaction,
  };
}

function removeHook<T>(hooks: Array<T | undefined>, hook: T): void {
  const index = hooks.indexOf(hook);
  if (index >= 0) hooks.splice(index, 1);
}

function cleanupInteractionAttributes(
  over: HTMLElement,
  selectEl: HTMLElement | null,
  originalSelectBackground: string,
): void {
  over.classList.remove('zoom-drag');
  delete over.dataset.visibleFrom;
  delete over.dataset.visibleTo;
  if (!selectEl) return;
  delete selectEl.dataset.testid;
  selectEl.style.background = originalSelectBackground;
}
