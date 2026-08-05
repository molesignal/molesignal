import * as React from 'react';

export interface ElementSize {
  width: number;
  height: number;
}

export function useElementSize(
  fallback: ElementSize,
): [React.RefObject<HTMLDivElement>, ElementSize] {
  const ref = React.useRef<HTMLDivElement>(null);
  const [size, setSize] = React.useState(fallback);

  React.useLayoutEffect(() => {
    const element = ref.current;
    if (!element) return;

    const measure = () => {
      const bounds = element.getBoundingClientRect();
      const width = bounds.width || element.clientWidth || fallback.width;
      const height = bounds.height || element.clientHeight || fallback.height;
      setSize((current) =>
        current.width === width && current.height === height
          ? current
          : { width, height },
      );
    };

    measure();
    if (typeof ResizeObserver === 'undefined') return;
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  }, [fallback.height, fallback.width]);

  return [ref, size];
}
