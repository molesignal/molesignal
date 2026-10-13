import * as React from 'react';

export const DESKTOP_MIN_WIDTH = 1024;

/** Below this the shell swaps the rail for a drawer; workbenches start with side panels folded. */
export const NARROW_MAX_WIDTH = 768;

/** For a side panel's initial state: on a phone the results need the width more than the field list. */
export function isNarrowViewport(): boolean {
  return typeof window !== 'undefined' && window.innerWidth < NARROW_MAX_WIDTH;
}

export function useViewportWidth(fallback = DESKTOP_MIN_WIDTH): number {
  const [width, setWidth] = React.useState(() =>
    typeof window === 'undefined' ? fallback : window.innerWidth,
  );
  React.useEffect(() => {
    const onResize = () => setWidth(window.innerWidth);
    window.addEventListener('resize', onResize);
    return () => window.removeEventListener('resize', onResize);
  }, []);
  return width;
}
