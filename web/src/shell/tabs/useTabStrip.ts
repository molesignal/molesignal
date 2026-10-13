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
// A strip this close to where it was sent has arrived. Where it is sent is
// worked out in whole pixels and a browser scrolls by fractions of one, so the
// end of what it can scroll may be a pixel short of that.
const ARRIVED = 2;

function sameBox(a: TabBox | null, b: TabBox | null): boolean {
  if (a === b) return true;
  if (!a || !b) return false;
  return a.left === b.left && a.top === b.top && a.width === b.width && a.height === b.height;
}

function prefersReducedMotion(): boolean {
  return window.matchMedia('(prefers-reduced-motion: reduce)').matches;
}

function activeTabOf(strip: HTMLElement | null): HTMLElement | null {
  return strip?.querySelector<HTMLElement>('[data-tab-active="true"]') ?? null;
}

/**
 * How far the tabs run on past the end of what the scroller shows.
 *
 * Read from the last tab and not from `scrollWidth`. The indicator is drawn in
 * the same scroll area and glides: when the tab it was under has become
 * narrower or is gone, it reaches past the last tab until it has caught up,
 * and for that long the area is wider than the tabs. Nothing is resized when
 * it arrives, so an edge that faded for it would stay faded.
 */
function tabsPastEnd(scroller: HTMLElement): number {
  const tabs = scroller.querySelectorAll<HTMLElement>('[data-tab-active]');
  const last = tabs[tabs.length - 1];
  if (!last) return 0;
  return last.getBoundingClientRect().right - scroller.getBoundingClientRect().right;
}

/**
 * Whether a tab is shown whole and clear of the fades: at each edge of the
 * strip, either no tab runs on beyond it, so nothing fades there, or the tab
 * keeps its distance from it.
 */
function sitsClear(scroller: HTMLElement, tab: HTMLElement): boolean {
  const start = tab.offsetLeft;
  const end = start + tab.offsetWidth;
  const shownFrom = scroller.scrollLeft;
  const shownTo = shownFrom + scroller.clientWidth;
  return (
    (shownFrom <= 1 || start >= shownFrom + FADE_CLEARANCE) &&
    (tabsPastEnd(scroller) <= 1 || end <= shownTo - FADE_CLEARANCE)
  );
}

/**
 * Everything a row of tabs needs besides its markup:
 *  - where the active tab is, so one indicator can slide to it;
 *  - which edges have more tabs beyond them, so those edges fade;
 *  - keeping the active tab in view when the strip scrolls;
 *  - arrow keys moving focus between the tabs.
 *
 * The first three are read again whenever a tab changes size, comes or goes,
 * whatever the cause: a name or a count that arrives later, another language,
 * a font that has just loaded. None of these resizes the strip itself.
 *
 * Every tab carries `data-tab-active`, `"true"` on the active one, on the box
 * the indicator is drawn under, and `data-tab` on the element that takes focus
 * (the same element, unless a tab holds more than its button). The content
 * element must be the offset parent of its tabs (`position: relative`). With
 * `activateOnArrow`, arrow keys also select the tab they land on (in-page
 * tabs); route tabs only move focus, since selecting one would navigate.
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
  // The box the indicator was last given: what tells a tab that has moved or
  // changed size from one that has not.
  const placed = React.useRef<TabBox | null>(null);
  // The strip keeps the active tab clear of its fades for as long as the tab
  // was clear of them where the strip was last left. A person who has scrolled
  // it under a fade or out of view has chosen what the strip shows: it stays
  // there, whatever the tabs do, until they scroll the tab clear again or open
  // another.
  const following = React.useRef(true);
  // Where a scroll this hook started is taking the strip, until it is there.
  // What the strip passes on the way, and where it arrives, is not where
  // anyone left it.
  const heading = React.useRef<number | null>(null);

  /** Reads where the active tab is. True when that is not where it was. */
  const measure = React.useCallback(() => {
    const active = activeTabOf(contentRef.current);
    const next = active
      ? {
          left: active.offsetLeft,
          top: active.offsetTop,
          width: active.offsetWidth,
          height: active.offsetHeight,
        }
      : null;
    if (sameBox(placed.current, next)) return false;
    placed.current = next;
    setBox(next);
    return true;
  }, []);

  const readEdges = React.useCallback(() => {
    const scroller = scrollerRef.current;
    if (!scroller) return;
    const next = {
      start: scroller.scrollLeft > 1,
      end: tabsPastEnd(scroller) > 1,
    };
    setEdges((previous) =>
      previous.start === next.start && previous.end === next.end ? previous : next,
    );
  }, []);

  /** Scrolls the active tab to the middle of the strip, unless it already sits clear. */
  const bringIntoView = React.useCallback(() => {
    const scroller = scrollerRef.current;
    const active = activeTabOf(scroller);
    if (!scroller || !active || typeof scroller.scrollTo !== 'function') return;
    if (sitsClear(scroller, active)) return;

    const centred = active.offsetLeft - (scroller.clientWidth - active.offsetWidth) / 2;
    const furthest = Math.max(0, scroller.scrollWidth - scroller.clientWidth);
    const left = Math.min(Math.max(0, centred), furthest);
    // Already as near as the strip can show it, as a tab wider than the strip
    // is. No scroll would come of asking, so none is waited for.
    if (Math.abs(left - scroller.scrollLeft) < 1) return;

    heading.current = left;
    scroller.scrollTo({ left, behavior: prefersReducedMotion() ? 'auto' : 'smooth' });
  }, []);

  React.useLayoutEffect(() => {
    measure();
  }, [activeKey, measure]);

  // The first placement is not animated: the indicator appears under the
  // active tab instead of sliding in from the left edge.
  React.useEffect(() => {
    if (!box || animated) return;
    const frame = window.requestAnimationFrame(() => setAnimated(true));
    return () => window.cancelAnimationFrame(frame);
  }, [box, animated]);

  React.useLayoutEffect(() => {
    const scroller = scrollerRef.current;
    const content = contentRef.current;
    if (!scroller || !content) return;

    const noteWhetherFollowing = () => {
      const active = activeTabOf(scroller);
      if (active) following.current = sitsClear(scroller, active);
    };
    const onScroll = () => {
      readEdges();
      if (heading.current === null) noteWhetherFollowing();
      else if (Math.abs(scroller.scrollLeft - heading.current) < ARRIVED) heading.current = null;
    };
    // A scroll of this hook's that has ended anywhere else was cut short, by a
    // person or by tabs that are gone: the strip was left where it stopped. A
    // browser without `scrollend` never says so, and the strip goes on
    // following until a scroll of its own next arrives.
    const onScrollEnd = () => {
      if (heading.current === null) return;
      heading.current = null;
      noteWhetherFollowing();
    };
    // The tabs, or the room they are given, are not what they were.
    const onLayout = () => {
      const moved = measure();
      readEdges();
      if (moved && following.current) bringIntoView();
    };

    readEdges();
    scroller.addEventListener('scroll', onScroll, { passive: true });
    scroller.addEventListener('scrollend', onScrollEnd);
    const stopListening = () => {
      scroller.removeEventListener('scroll', onScroll);
      scroller.removeEventListener('scrollend', onScrollEnd);
    };
    if (typeof ResizeObserver === 'undefined') return stopListening;

    // A strip that does not fit is as wide as its scroller whatever its tabs
    // do, so each tab is watched for itself, and the list of them for tabs
    // that come and go.
    const sizes = new ResizeObserver(onLayout);
    sizes.observe(scroller);
    sizes.observe(content);
    const watched = new Set<Element>();
    const watchTabs = () => {
      const tabs = new Set<Element>(content.querySelectorAll('[data-tab-active]'));
      for (const tab of watched) {
        if (tabs.has(tab)) continue;
        sizes.unobserve(tab);
        watched.delete(tab);
      }
      for (const tab of tabs) {
        if (watched.has(tab)) continue;
        sizes.observe(tab);
        watched.add(tab);
      }
    };
    watchTabs();
    const tabList = new MutationObserver(() => {
      watchTabs();
      onLayout();
    });
    tabList.observe(content, { childList: true, subtree: true });

    return () => {
      stopListening();
      sizes.disconnect();
      tabList.disconnect();
    };
  }, [measure, readEdges, bringIntoView]);

  React.useEffect(() => {
    following.current = true;
    bringIntoView();
  }, [activeKey, bringIntoView]);

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
