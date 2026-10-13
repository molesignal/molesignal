import { useEffect, useLayoutEffect, useState, type RefObject } from 'react';

import { cn } from '@/shell/lib/cn';

const INDICATOR_HEIGHT = 16;

export interface IndicatorBox {
  top: number;
  height: number;
}

/**
 * Where the active navigation row is, measured inside the scrolling container, so
 * one indicator can glide to it. NavLink marks the active row with
 * `aria-current="page"`; nothing else has to opt in. The first placement is not
 * animated, so the indicator appears beside the active row instead of sliding
 * down from the top of the rail on load.
 */
export function useNavIndicator(
  navRef: RefObject<HTMLElement>,
  contentRef: RefObject<HTMLElement>,
  layoutKey: string,
) {
  const [box, setBox] = useState<IndicatorBox | null>(null);
  const [animated, setAnimated] = useState(false);

  useLayoutEffect(() => {
    const nav = navRef.current;
    if (!nav) return;
    const measure = () => {
      const active = nav.querySelector<HTMLElement>('a[aria-current="page"]');
      if (!active) {
        setBox(null);
        return;
      }
      const activeRect = active.getBoundingClientRect();
      const next = {
        top: activeRect.top - nav.getBoundingClientRect().top + nav.scrollTop,
        height: activeRect.height,
      };
      setBox((previous) =>
        previous && previous.top === next.top && previous.height === next.height
          ? previous
          : next,
      );
    };
    measure();
    if (typeof ResizeObserver === 'undefined') return;
    // Rows move when permissions load, an item is pinned or the rail expands.
    const observer = new ResizeObserver(measure);
    if (contentRef.current) observer.observe(contentRef.current);
    return () => observer.disconnect();
  }, [navRef, contentRef, layoutKey]);

  useEffect(() => {
    if (!box || animated) return;
    const frame = window.requestAnimationFrame(() => setAnimated(true));
    return () => window.cancelAnimationFrame(frame);
  }, [box, animated]);

  return { box, animated };
}

/** The glide: a short signal-coloured bar on the navigation's left edge. */
export function NavIndicator({
  box,
  animated,
  testId,
}: {
  box: IndicatorBox | null;
  animated: boolean;
  testId?: string;
}) {
  return (
    <span
      aria-hidden
      data-testid={testId}
      className={cn(
        'pointer-events-none absolute left-0 top-0 w-[3px] rounded-r-full bg-[image:var(--signal-gradient-v)]',
        animated && 'transition-[transform,opacity] duration-300 ease-expo-out',
      )}
      style={{
        height: INDICATOR_HEIGHT,
        opacity: box ? 1 : 0,
        transform: `translateY(${box ? box.top + (box.height - INDICATOR_HEIGHT) / 2 : 0}px)`,
      }}
    />
  );
}
