import * as React from 'react';

/**
 * Starts loading the shapes of the world, the larger part of a map. A map is drawn when the
 * numbers it shows have come, which is after the page opened: with the shapes already on their
 * way, the two are fetched side by side and not one after the other. It does nothing the second
 * time.
 */
export function preloadWorldMap(): void {
  void import('./world').catch(() => undefined);
}

/**
 * Starts loading the shapes of the world when a page that is likely to draw a map opens, in the
 * idle time after it, so that it takes nothing from the page's own first requests.
 */
export function usePreloadWorldMap(enabled = true): void {
  React.useEffect(() => {
    if (!enabled) return undefined;

    if (typeof window.requestIdleCallback === 'function') {
      const handle = window.requestIdleCallback(preloadWorldMap, { timeout: 2_000 });

      return () => window.cancelIdleCallback(handle);
    }

    const timer = setTimeout(preloadWorldMap, 200);

    return () => clearTimeout(timer);
  }, [enabled]);
}
