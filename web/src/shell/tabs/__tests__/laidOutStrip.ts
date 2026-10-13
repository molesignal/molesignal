import { act, fireEvent } from '@testing-library/react';
import { onTestFinished, vi } from 'vitest';

/**
 * A tab strip as a browser lays it out and scrolls it, for tests of anything
 * built on `useTabStrip`: jsdom lays nothing out, observes no sizes and does
 * not scroll. Everything here is undone when the test that asked for it ends.
 *
 * The scroller is the `.tabs-scroll` element and a tab is a `[data-tab-active]`
 * box in it, as in every strip.
 */

const box = (left: number, right: number) => ({ left, right, width: right - left }) as DOMRect;

const tabsOf = (scroller: Element) => Array.from(scroller.querySelectorAll('[data-tab-active]'));

/**
 * The tabs sit side by side from the start of a scroller that shows `shown`
 * pixels, each 100px wide unless `widths` says otherwise. `scrollWidth` is how
 * wide the browser calls the scroll area: the tabs, and the indicator where it
 * reaches past them.
 *
 * What comes back is the layout itself: a test that changes a width in it has
 * changed what the browser would measure from then on.
 */
export function layOut({ shown, scrollWidth }: { shown: number; scrollWidth?: number }) {
  const laid = { shown, widths: [] as number[] };
  const widthOf = (index: number) => laid.widths[index] ?? 100;
  /** Where a tab begins and ends, counted from the start of the first tab. */
  const spanOf = (element: Element) => {
    const scroller = element.closest('.tabs-scroll');
    const index = scroller ? tabsOf(scroller).indexOf(element) : -1;
    if (index === -1) return null;

    let left = 0;
    for (let before = 0; before < index; before += 1) left += widthOf(before);
    return { left, right: left + widthOf(index) };
  };

  const measured = [
    vi.spyOn(Element.prototype, 'getBoundingClientRect').mockImplementation(function (this: Element) {
      const scroller = this.closest('.tabs-scroll');
      if (!scroller) return box(0, 0);
      if (this === scroller) return box(0, laid.shown);

      const span = spanOf(this);
      return span ? box(span.left - scroller.scrollLeft, span.right - scroller.scrollLeft) : box(0, 0);
    }),
    vi.spyOn(HTMLElement.prototype, 'offsetLeft', 'get').mockImplementation(function (this: HTMLElement) {
      return spanOf(this)?.left ?? 0;
    }),
    vi.spyOn(HTMLElement.prototype, 'offsetWidth', 'get').mockImplementation(function (this: HTMLElement) {
      const span = spanOf(this);
      return span ? span.right - span.left : 0;
    }),
    vi.spyOn(Element.prototype, 'clientWidth', 'get').mockImplementation(function (this: Element) {
      return this.classList.contains('tabs-scroll') ? laid.shown : 0;
    }),
    vi.spyOn(Element.prototype, 'scrollWidth', 'get').mockImplementation(function (this: Element) {
      if (!this.classList.contains('tabs-scroll')) return 0;
      return scrollWidth ?? tabsOf(this).reduce((total, _tab, index) => total + widthOf(index), 0);
    }),
  ];
  onTestFinished(() => {
    for (const spy of measured) spy.mockRestore();
  });

  return laid;
}

/**
 * The browser's ResizeObserver. An element reports nothing by itself here:
 * the function that comes back says that one has changed size.
 */
export function watchSizes() {
  const observers = new Set<{ watched: Set<Element>; report: () => void }>();

  vi.stubGlobal(
    'ResizeObserver',
    class {
      private readonly observer: { watched: Set<Element>; report: () => void };

      constructor(report: () => void) {
        this.observer = { watched: new Set(), report };
        observers.add(this.observer);
      }

      observe(element: Element) {
        this.observer.watched.add(element);
      }

      unobserve(element: Element) {
        this.observer.watched.delete(element);
      }

      disconnect() {
        observers.delete(this.observer);
      }
    },
  );
  onTestFinished(() => {
    vi.unstubAllGlobals();
  });

  return function resized(element: Element) {
    act(() => {
      for (const observer of observers) {
        if (observer.watched.has(element)) observer.report();
      }
    });
  };
}

/**
 * `scrollTo`. A strip scrolled with it is at once as near to where it was sent
 * as it can scroll, and says so, the way a browser does at the end of a glide.
 * `onItsWay` leaves the strip where it was instead: a glide that has only
 * begun.
 */
export function scrolling({ onItsWay = false } = {}) {
  const scrollTo = vi.fn(function (this: Element, { left = 0 }: ScrollToOptions) {
    const arrived = Math.max(0, Math.min(left, this.scrollWidth - this.clientWidth));
    if (onItsWay || arrived === this.scrollLeft) return;

    this.scrollLeft = arrived;
    fireEvent.scroll(this);
    fireEvent(this, new Event('scrollend'));
  });

  Object.defineProperty(Element.prototype, 'scrollTo', { configurable: true, value: scrollTo });
  onTestFinished(() => {
    Reflect.deleteProperty(Element.prototype, 'scrollTo');
  });

  return scrollTo;
}

/** What a person does to a strip with a wheel or a finger: it is at `left`, and has stopped there. */
export function scrollByHand(scroller: Element, left: number) {
  scroller.scrollLeft = left;
  fireEvent.scroll(scroller);
  fireEvent(scroller, new Event('scrollend'));
}

/** The strip in `container`: its scroller, its tabs, and how much of each end is faded out. */
export function stripIn(container: ParentNode) {
  const scroller = container.querySelector<HTMLElement>('.tabs-scroll')!;

  return {
    scroller,
    tabs: tabsOf(scroller) as HTMLElement[],
    fades: () => ({
      start: scroller.style.getPropertyValue('--tabs-fade-start'),
      end: scroller.style.getPropertyValue('--tabs-fade-end'),
    }),
  };
}
