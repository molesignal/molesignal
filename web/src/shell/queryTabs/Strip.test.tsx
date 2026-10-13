import { act, cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import i18n from '@/i18n';
import { layOut, scrolling, stripIn, watchSizes } from '@/shell/tabs/__tests__/laidOutStrip';
import { TooltipProvider } from '@/shell/ui/tooltip';

import { QueryTabStrip, type QueryTabStripItem } from './Strip';

function tab(id: string, overrides: Partial<QueryTabStripItem> = {}): QueryTabStripItem {
  return {
    id,
    name: null,
    ordinal: Number(id.replace(/\D/g, '')) || 1,
    running: false,
    tabId: `tab-${id}`,
    panelId: `panel-${id}`,
    ...overrides,
  };
}

type StripProps = Partial<React.ComponentProps<typeof QueryTabStrip>>;

function renderStrip(overrides: StripProps = {}) {
  const handlers = {
    onSelect: vi.fn(),
    onAdd: vi.fn(),
    onClose: vi.fn(),
    onCloseOthers: vi.fn(),
    onDuplicate: vi.fn(),
    onRename: vi.fn(),
  };
  const strip = (props: StripProps) => (
    <TooltipProvider>
      <QueryTabStrip
        tabs={[tab('t1', { name: 'error logs' }), tab('t2')]}
        activeId="t1"
        canAdd
        limit={10}
        {...handlers}
        {...props}
      />
    </TooltipProvider>
  );
  const { rerender } = render(strip(overrides));

  return {
    ...handlers,
    /** The strip as its explorer renders it again: a tab named, running, opened or closed. */
    update: (props: StripProps) => rerender(strip({ ...overrides, ...props })),
  };
}

beforeEach(async () => {
  await i18n.changeLanguage('en-us');
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

describe('QueryTabStrip', () => {
  it('names each tab, numbering the ones nobody named, and marks the one in front', () => {
    renderStrip();

    const tabs = screen.getAllByRole('tab');
    expect(tabs.map((item) => item.textContent)).toEqual([
      expect.stringContaining('error logs'),
      expect.stringContaining('Query 2'),
    ]);
    expect(tabs[0]?.getAttribute('aria-selected')).toBe('true');
    expect(tabs[1]?.getAttribute('aria-selected')).toBe('false');
    expect(tabs[0]?.getAttribute('aria-controls')).toBe('panel-t1');
    // Only the tab in front is in the tab order; arrows move between them.
    expect(tabs.map((item) => item.tabIndex)).toEqual([0, -1]);
    expect(screen.getByRole('tablist').getAttribute('aria-label')).toBe('Open queries');
  });

  it('selects, adds and closes', () => {
    const handlers = renderStrip();

    fireEvent.click(screen.getAllByRole('tab')[1]!);
    expect(handlers.onSelect).toHaveBeenCalledWith('t2');

    fireEvent.click(screen.getByRole('button', { name: 'New tab' }));
    expect(handlers.onAdd).toHaveBeenCalledTimes(1);

    fireEvent.click(screen.getByRole('button', { name: 'Close Query 2' }));
    expect(handlers.onClose).toHaveBeenCalledWith('t2');
    expect(handlers.onSelect).toHaveBeenCalledTimes(1);
  });

  it('closes a tab on a middle click', () => {
    const handlers = renderStrip();

    fireEvent(
      document.querySelector('[data-query-tab="t2"]')!,
      new MouseEvent('auxclick', { bubbles: true, button: 1 }),
    );

    expect(handlers.onClose).toHaveBeenCalledWith('t2');
  });

  it('does not offer to close the only tab', () => {
    const handlers = renderStrip({ tabs: [tab('t1')], activeId: 't1' });

    expect(screen.queryByRole('button', { name: /^Close / })).toBeNull();
    fireEvent(
      document.querySelector('[data-query-tab="t1"]')!,
      new MouseEvent('auxclick', { bubbles: true, button: 1 }),
    );
    expect(handlers.onClose).not.toHaveBeenCalled();
  });

  it('renames a tab in place on a double click and commits on Enter', () => {
    const handlers = renderStrip();

    fireEvent.doubleClick(screen.getAllByRole('tab')[0]!);
    const field = screen.getByRole('textbox', { name: 'Tab name' }) as HTMLInputElement;
    expect(field.value).toBe('error logs');

    fireEvent.change(field, { target: { value: 'Checkout errors' } });
    fireEvent.keyDown(field, { key: 'Enter' });

    expect(handlers.onRename).toHaveBeenCalledWith('t1', 'Checkout errors');
    expect(screen.queryByRole('textbox', { name: 'Tab name' })).toBeNull();
  });

  it('leaves the name alone when renaming is cancelled', () => {
    const handlers = renderStrip();

    fireEvent.keyDown(screen.getAllByRole('tab')[0]!, { key: 'F2' });
    const field = screen.getByRole('textbox', { name: 'Tab name' });
    fireEvent.change(field, { target: { value: 'something else' } });
    fireEvent.keyDown(field, { key: 'Escape' });

    expect(handlers.onRename).not.toHaveBeenCalled();
    expect(screen.getAllByRole('tab')[0]?.textContent).toContain('error logs');
  });

  it('marks a tab whose query is running', () => {
    renderStrip({ tabs: [tab('t1'), tab('t2', { running: true })] });

    const [first, second] = screen.getAllByRole('tab');
    expect(first?.querySelector('[data-query-tab-running]')).toBeNull();
    expect(second?.querySelector('[data-query-tab-running]')?.getAttribute('aria-label')).toBe(
      'Running',
    );
  });

  it('stops offering a new tab at the limit', () => {
    const handlers = renderStrip({ canAdd: false });

    const add = screen.getByRole('button', { name: 'New tab' }) as HTMLButtonElement;
    expect(add.disabled).toBe(true);
    fireEvent.click(add);
    expect(handlers.onAdd).not.toHaveBeenCalled();
  });
});

// How a strip is laid out here, and what stands in for the browser, is in `laidOutStrip`.
describe('the edges of QueryTabStrip', () => {
  const fadeAtEnd = () => stripIn(document.body).fades().end;

  it('fades its end while a tab is cut off there, whichever tab is in front', () => {
    layOut({ shown: 150, scrollWidth: 200 });
    renderStrip();

    expect(fadeAtEnd()).toBe('28px');
  });

  it('does not take the surface for a tab where it reaches past the last one', () => {
    // Both tabs fit. The one in front has just lost its running mark, and the
    // surface under it is still as wide as the tab was.
    layOut({ shown: 200, scrollWidth: 212 });
    renderStrip({ activeId: 't2' });

    expect(fadeAtEnd()).toBe('0px');
  });

  it('fades its end when the name a tab is given makes it too wide for the strip', () => {
    const resized = watchSizes();
    const laid = layOut({ shown: 200 });
    const { update } = renderStrip();
    expect(fadeAtEnd()).toBe('0px');

    // The query behind has been given a stream, and its tab is named after it.
    update({ tabs: [tab('t1', { name: 'error logs' }), tab('t2', { name: 'application_logs' })] });
    laid.widths[1] = 132;
    resized(stripIn(document.body).tabs[1]!);

    expect(fadeAtEnd()).toBe('28px');
  });

  it('fades its end for a tab opened behind the others, and not once that tab is closed', async () => {
    watchSizes();
    layOut({ shown: 250 });
    const { update } = renderStrip();
    expect(fadeAtEnd()).toBe('0px');

    await act(async () => {
      update({ tabs: [tab('t1', { name: 'error logs' }), tab('t2'), tab('t3')] });
    });
    expect(fadeAtEnd()).toBe('28px');

    await act(async () => {
      update({ tabs: [tab('t1', { name: 'error logs' }), tab('t2')] });
    });
    expect(fadeAtEnd()).toBe('0px');
  });
});

describe('the tab in front of QueryTabStrip', () => {
  it('stays in view when its name, and then its running mark, make it wider', () => {
    const resized = watchSizes();
    scrolling();
    const laid = layOut({ shown: 250 });
    const tabs = (last: Partial<QueryTabStripItem>) => [tab('t1'), tab('t2'), tab('t3', last)];
    const { update } = renderStrip({ tabs: tabs({}), activeId: 't3' });
    const { scroller } = stripIn(document.body);
    // A new tab, the last of three 100px ones in a strip that shows 250px.
    expect(scroller.scrollLeft).toBe(50);

    // Its explorer has picked a stream: "Query 3" is now the name of the stream.
    update({ tabs: tabs({ name: 'application_logs' }) });
    laid.widths[2] = 132;
    resized(stripIn(document.body).tabs[2]!);
    expect(scroller.scrollLeft).toBe(82);

    update({ tabs: tabs({ name: 'application_logs', running: true }) });
    laid.widths[2] = 144;
    resized(stripIn(document.body).tabs[2]!);
    expect(scroller.scrollLeft).toBe(94);
  });
});
