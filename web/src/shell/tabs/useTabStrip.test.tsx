import { act, cleanup, fireEvent, render } from '@testing-library/react';
import type * as React from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { layOut, scrollByHand, scrolling, stripIn, watchSizes } from './__tests__/laidOutStrip';
import { ModuleTabs } from './ModuleTabs';
import { ViewTabs } from './ViewTabs';

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

const ITEMS = [
  { key: 'one', label: 'One' },
  { key: 'two', label: 'Two' },
  { key: 'three', label: 'Three' },
];

// The strips of this directory; the query tabs have the same checks beside them.
const STRIPS: Array<[string, (active: string, items?: typeof ITEMS) => React.ReactElement]> = [
  [
    'ModuleTabs',
    (active, items = ITEMS) => (
      <ModuleTabs label="Views" items={items} value={active} onValueChange={vi.fn()} />
    ),
  ],
  [
    'ViewTabs',
    (active, items = ITEMS) => (
      <ViewTabs label="Views" items={items} value={active} onValueChange={vi.fn()} />
    ),
  ],
];

describe.each(STRIPS)('the edges of %s', (_name, strip) => {
  it('fades neither end of tabs that fit', () => {
    layOut({ shown: 300 });
    const { container } = render(strip('one'));

    expect(stripIn(container).fades()).toEqual({ start: '0px', end: '0px' });
  });

  it('fades the end that has tabs beyond it, whichever tab is active', () => {
    layOut({ shown: 250 });
    const { container } = render(strip('one'));
    const { scroller, fades } = stripIn(container);
    expect(fades()).toEqual({ start: '0px', end: '28px' });

    scroller.scrollLeft = 20;
    fireEvent.scroll(scroller);
    expect(fades()).toEqual({ start: '28px', end: '28px' });

    scroller.scrollLeft = 50;
    fireEvent.scroll(scroller);
    expect(fades()).toEqual({ start: '28px', end: '0px' });
  });

  it('does not take an indicator that reaches past the last tab for a tab', () => {
    // The tabs fit. The last one has just become narrower, and the indicator
    // under it is still as wide as the tab was.
    layOut({ shown: 300, scrollWidth: 340 });
    const { container } = render(strip('three'));

    expect(stripIn(container).fades()).toEqual({ start: '0px', end: '0px' });
  });

  // A strip that does not fit, or that fills its row, is as wide as its
  // scroller whatever its tabs do: nothing about the strip itself says that a
  // tab has grown, or that there is one more or one fewer.
  it('fades its end as soon as a tab has become too wide for the strip', () => {
    const resized = watchSizes();
    const laid = layOut({ shown: 300 });
    const { container } = render(strip('one'));
    const { tabs, fades } = stripIn(container);
    expect(fades()).toEqual({ start: '0px', end: '0px' });

    // Its label came in another language, or its count gained a digit.
    laid.widths[1] = 180;
    resized(tabs[1]!);

    expect(fades()).toEqual({ start: '0px', end: '28px' });
  });

  it('fades its end for a tab that joins the strip later, and watches that tab too', async () => {
    const resized = watchSizes();
    const laid = layOut({ shown: 250 });
    const { container, rerender } = render(strip('one', ITEMS.slice(0, 2)));
    const { scroller, fades } = stripIn(container);
    expect(fades()).toEqual({ start: '0px', end: '0px' });

    await act(async () => {
      rerender(strip('one', ITEMS));
    });
    expect(fades()).toEqual({ start: '0px', end: '28px' });

    // Scrolled to its end the strip shows the new tab whole, until that tab grows.
    scrollByHand(scroller, 50);
    expect(fades()).toEqual({ start: '28px', end: '0px' });

    laid.widths[2] = 140;
    resized(stripIn(container).tabs[2]!);
    expect(fades()).toEqual({ start: '28px', end: '28px' });
  });

  it('stops fading its end once the tab beyond it has gone', async () => {
    watchSizes();
    layOut({ shown: 250 });
    const { container, rerender } = render(strip('one'));
    const { fades } = stripIn(container);
    expect(fades()).toEqual({ start: '0px', end: '28px' });

    await act(async () => {
      rerender(strip('one', ITEMS.slice(0, 2)));
    });

    expect(fades()).toEqual({ start: '0px', end: '0px' });
  });
});

describe.each(STRIPS)('the active tab of %s', (_name, strip) => {
  /** Three tabs in a strip that shows 250px, the last one active and brought into view. */
  function atItsEnd(options?: { onItsWay: boolean }) {
    const resized = watchSizes();
    const scrollTo = scrolling(options);
    const laid = layOut({ shown: 250 });
    const { container, rerender } = render(strip('three'));

    return { resized, scrollTo, laid, rerender, ...stripIn(container) };
  }

  const gliding = expect.objectContaining({ behavior: 'smooth' });

  it('is brought into view when it becomes active, wherever the strip was left', () => {
    const { scrollTo, scroller, rerender } = atItsEnd();
    expect(scroller.scrollLeft).toBe(50);
    expect(scrollTo).toHaveBeenLastCalledWith(gliding);

    scrollByHand(scroller, 0);
    rerender(strip('one'));
    expect(scroller.scrollLeft).toBe(0);

    rerender(strip('three'));
    expect(scroller.scrollLeft).toBe(50);
  });

  it('is brought back when it has become wider than the strip was showing', () => {
    const { resized, scrollTo, laid, scroller, tabs, fades } = atItsEnd();

    // The name it was waiting for has arrived: 60px more, all of them out of view.
    laid.widths[2] = 160;
    resized(tabs[2]!);

    expect(scroller.scrollLeft).toBe(110);
    expect(scrollTo).toHaveBeenLastCalledWith(gliding);
    expect(fades()).toEqual({ start: '28px', end: '0px' });
  });

  it('is brought back when a tab before it has pushed it out of view', () => {
    const { resized, laid, scroller, tabs } = atItsEnd();

    laid.widths[0] = 130;
    resized(tabs[0]!);

    expect(scroller.scrollLeft).toBe(80);
  });

  it('is brought back without a glide for a person who has asked for less motion', () => {
    vi.spyOn(window, 'matchMedia').mockImplementation(
      (query) => ({ matches: query.includes('prefers-reduced-motion'), media: query }) as MediaQueryList,
    );
    const { resized, scrollTo, laid, scroller, tabs } = atItsEnd();

    laid.widths[2] = 160;
    resized(tabs[2]!);

    expect(scroller.scrollLeft).toBe(110);
    expect(scrollTo).toHaveBeenLastCalledWith(expect.objectContaining({ behavior: 'auto' }));
  });

  it('is left where it is while it still sits clear of the fades', () => {
    const { resized, laid, scroller, tabs } = atItsEnd();

    // The second tab has lost its count. There is 10px less to scroll, and the
    // browser has pulled the strip back with its tabs: the active one is as
    // clear of both edges as it was.
    laid.widths[1] = 90;
    scroller.scrollLeft = 40;
    resized(tabs[1]!);

    expect(scroller.scrollLeft).toBe(40);
  });

  it('is not chased by a strip a person has scrolled away from it', () => {
    const { resized, laid, scroller, tabs } = atItsEnd();

    // Back to the first tab: the active one is half out of view, and meant to be.
    scrollByHand(scroller, 0);
    laid.widths[2] = 160;
    resized(tabs[2]!);
    expect(scroller.scrollLeft).toBe(0);

    // Scrolled to where the tab sits clear again, the strip keeps it there.
    scrollByHand(scroller, 110);
    laid.widths[2] = 200;
    resized(tabs[2]!);
    expect(scroller.scrollLeft).toBe(150);
  });

  it('is still followed when it changes while the strip is gliding to it', () => {
    const { resized, scrollTo, laid, scroller, tabs } = atItsEnd({ onItsWay: true });
    expect(scrollTo).toHaveBeenCalledTimes(1);

    // Part of the way there. Nobody put the strip here: it is passing through.
    scroller.scrollLeft = 20;
    fireEvent.scroll(scroller);
    laid.widths[2] = 160;
    resized(tabs[2]!);

    // Sent on to where the tab ends now, not left to stop where it ended before.
    expect(scrollTo).toHaveBeenCalledTimes(2);
    expect(scrollTo).toHaveBeenLastCalledWith({ left: 110, behavior: 'smooth' });
  });

  it('is still followed when it changes before the strip has said that it arrived', () => {
    const { resized, scrollTo, laid, scroller, tabs } = atItsEnd({ onItsWay: true });

    // A browser reports a scroll a frame after it has happened. By then the tab
    // is wider, and the strip is where it was sent for the narrower one: that
    // is not somebody having scrolled it there.
    scroller.scrollLeft = 50;
    laid.widths[2] = 160;
    fireEvent.scroll(scroller);
    fireEvent(scroller, new Event('scrollend'));
    resized(tabs[2]!);

    expect(scrollTo).toHaveBeenCalledTimes(2);
    expect(scrollTo).toHaveBeenLastCalledWith({ left: 110, behavior: 'smooth' });
  });

  it('is not chased once a person has cut the glide to it short', () => {
    const { resized, scrollTo, laid, scroller, tabs } = atItsEnd({ onItsWay: true });

    // Part of the way there the person takes the strip back to its start.
    scroller.scrollLeft = 20;
    fireEvent.scroll(scroller);
    scroller.scrollLeft = 0;
    fireEvent.scroll(scroller);
    fireEvent(scroller, new Event('scrollend'));
    laid.widths[2] = 160;
    resized(tabs[2]!);

    expect(scrollTo).toHaveBeenCalledTimes(1);
  });
});
