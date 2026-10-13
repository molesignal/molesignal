import * as React from 'react';

/**
 * The width of an element as it is laid out, kept as it changes. `null` until
 * the element has been measured once; `0` where nothing is laid out at all.
 */
export function useWidth(element: HTMLElement | null): number | null {
  const [width, setWidth] = React.useState<number | null>(null);

  React.useLayoutEffect(() => {
    if (!element) return undefined;

    const measure = () => setWidth(Math.round(element.getBoundingClientRect().width));
    measure();
    if (typeof ResizeObserver === 'undefined') return undefined;

    const observer = new ResizeObserver(measure);
    observer.observe(element);

    return () => observer.disconnect();
  }, [element]);

  return width;
}
