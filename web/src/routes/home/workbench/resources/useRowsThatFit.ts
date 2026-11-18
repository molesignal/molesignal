import * as React from 'react';

/** The heights `TableShell` draws a row and its head at. */
const ROW_HEIGHT = 36;
const HEAD_HEIGHT = 34;
/** Before anything is measured (a first paint, a test): what a laptop screen holds. */
const FALLBACK_ROWS = 8;

/**
 * How many table rows fit the element's height below the table's head. The element is as tall as
 * its column leaves it, so the list ends with the column instead of running past it.
 */
export function useRowsThatFit(ref: React.RefObject<HTMLElement | null>): number {
  const [rows, setRows] = React.useState(FALLBACK_ROWS);

  React.useLayoutEffect(() => {
    const element = ref.current;
    if (!element || typeof ResizeObserver === 'undefined') return;

    const update = () => {
      const height = element.clientHeight;
      if (height > 0) setRows(Math.max(1, Math.floor((height - HEAD_HEIGHT) / ROW_HEIGHT)));
    };
    update();
    const observer = new ResizeObserver(update);
    observer.observe(element);
    return () => observer.disconnect();
  }, [ref]);

  return rows;
}
