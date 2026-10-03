import * as React from 'react';

export interface TabBox {
  left: number;
  top: number;
  width: number;
  height: number;
}

const FADE_LENGTH = 28;
// A tab this close to the edge of a scrolled strip sits under the fade.
const FADE_CLEARANCE = FADE_LENGTH + 4;

function sameBox(a: TabBox | null, b: TabBox | null): boolean {
  if (a === b) return true;
  if (!a || !b) return false;
  return a.left === b.left && a.top === b.top && a.width === b.width && a.height === b.height;
}

function prefersReducedMotion(): boolean {
  return window.matchMedia('(prefers-reduced-motion: reduce)').matches;
}

/**
 * Everything a row of tabs needs besides its markup:
 *  - where the active tab is, so one indicator can slide to it;
 *  - which edges have more tabs beyond them, so those edges fade;
 *  - keeping the active tab in view when the strip scrolls;
 *  - arrow keys moving focus between the tabs.
 *
 * Tabs mark themselves with `data-tab`, and the active one with
 * `data-tab-active="true"`. The content element must be the offset parent of
 * its tabs (`position: relative`). With `activateOnArrow`, arrow keys also
 * select the tab they land on (in-page tabs); route tabs only move focus, since
 * selecting one would navigate.
 */
export function useTabStrip(
  activeKey: string | undefined,
  { activateOnArrow = false }: { activateOnArrow?: boolean } = {},
) {
  const scrollerRef = React.useRef<HTMLDivElement>(null);
  const contentRef = React.useRef<HTMLDivElement>(null);
  const [box, setBox] = React.useState<TabBox | null>(null);
  const [animated, setAnimated] = React.useState(false);
  const [edges, setEdges] = React.useState({ start: false, end: false });

  React.useLayoutEffect(() => {
    const content = contentRef.current;
    if (!content) return;
    const measure = () => {
      const active = content.querySelector<HTMLElement>('[data-tab-active="true"]');
      const next = active
        ? {
            left: active.offsetLeft,
            top: active.offsetTop,
            width: active.offsetWidth,
            height: active.offsetHeight,
          }
        : null;
      setBox((previous) => (sameBox(previous, next) ? previous : next));
    };
    measure();
    if (typeof ResizeObserver === 'undefined') return;
    const observer = new ResizeObserver(measure);
    observer.observe(content);
    for (const tab of content.querySelectorAll('[data-tab]')) observer.observe(tab);
    return () => observer.disconnect();
  }, [activeKey]);

  // The first placement is not animated: the indicator appears under the
  // active tab instead of sliding in from the left edge.
  React.useEffect(() => {
    if (!box || animated) return;
    const frame = window.requestAnimationFrame(() => setAnimated(true));
    return () => window.cancelAnimationFrame(frame);
  }, [box, animated]);

  React.useLayoutEffect(() => {
    const scroller = scrollerRef.current;
    if (!scroller) return;
    const update = () => {
      const next = {
        start: scroller.scrollLeft > 1,
        end: scroller.scrollLeft + scroller.clientWidth < scroller.scrollWidth - 1,
      };
      setEdges((previous) =>
        previous.start === next.start && previous.end === next.end ? previous : next,
      );
    };
    update();
    scroller.addEventListener('scroll', update, { passive: true });
    if (typeof ResizeObserver === 'undefined') {
      return () => scroller.removeEventListener('scroll', update);
    }
    const observer = new ResizeObserver(update);
    observer.observe(scroller);
    if (contentRef.current) observer.observe(contentRef.current);
    return () => {
      scroller.removeEventListener('scroll', update);
      observer.disconnect();
    };
  }, []);

  React.useEffect(() => {
    const scroller = scrollerRef.current;
    const active = scroller?.querySelector<HTMLElement>('[data-tab-active="true"]');
    if (!scroller || !active || typeof scroller.scrollTo !== 'function') return;
    const start = active.offsetLeft;
    const end = start + active.offsetWidth;
    const comfortable =
      start >= scroller.scrollLeft + FADE_CLEARANCE &&
      end <= scroller.scrollLeft + scroller.clientWidth - FADE_CLEARANCE;
    if (comfortable) return;
    scroller.scrollTo({
      left: Math.max(0, start - (scroller.clientWidth - active.offsetWidth) / 2),
      behavior: prefersReducedMotion() ? 'auto' : 'smooth',
    });
  }, [activeKey]);

  const onKeyDown = React.useCallback((event: React.KeyboardEvent<HTMLElement>) => {
    const tabs = Array.from(
      event.currentTarget.querySelectorAll<HTMLElement>('[data-tab]'),
    );
    const current = tabs.indexOf(document.activeElement as HTMLElement);
    if (current === -1) return;
    const target =
      event.key === 'ArrowRight'
        ? (current + 1) % tabs.length
        : event.key === 'ArrowLeft'
          ? (current - 1 + tabs.length) % tabs.length
          : event.key === 'Home'
            ? 0
            : event.key === 'End'
              ? tabs.length - 1
              : -1;
    if (target === -1) return;
    event.preventDefault();
    tabs[target]?.focus();
    if (activateOnArrow) tabs[target]?.click();
  }, [activateOnArrow]);

  const fadeStyle = {
    '--tabs-fade-start': edges.start ? `${FADE_LENGTH}px` : '0px',
    '--tabs-fade-end': edges.end ? `${FADE_LENGTH}px` : '0px',
  } as React.CSSProperties;

  return { scrollerRef, contentRef, box, animated, fadeStyle, onKeyDown };
}
