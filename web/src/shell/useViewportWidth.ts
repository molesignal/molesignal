import * as React from 'react';

export const DESKTOP_MIN_WIDTH = 1024;

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
