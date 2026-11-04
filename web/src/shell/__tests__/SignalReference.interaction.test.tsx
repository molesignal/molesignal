import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, describe, expect, it, vi } from 'vitest';

import i18n from '@/i18n';
import { isNestedControlClick } from '@/shell/lib/nestedControl';
import { SignalReference } from '@/shell/SignalReference';
import { useFiltersStore } from '@/stores/useFiltersStore';
import { DEFAULT_WINDOW, useTimeStore } from '@/stores/useTimeStore';

afterEach(() => {
  cleanup();
  useFiltersStore.getState().clearFilters();
  useTimeStore.getState().setWindow(DEFAULT_WINDOW);
});

describe('SignalReference interaction', () => {
  it('opens an exact trace route with the inherited investigation time', async () => {
    await i18n.changeLanguage('en-us');
    useTimeStore.getState().setWindow({
      from: '2026-08-14T05:00:00.000Z',
      to: '2026-08-14T06:00:00.000Z',
      mode: 'absolute',
    });
    render(
      <MemoryRouter>
        <SignalReference type="trace_id" value="trace-1" />
      </MemoryRouter>,
    );

    fireEvent.click(screen.getByRole('button', { name: /trace-1/ }));
    const link = await screen.findByRole('link', { name: /Open current trace/ });
    const url = new URL(link.getAttribute('href')!, 'https://molesignal.local');

    expect(url.pathname).toBe('/traces/trace-1');
    expect(url.searchParams.get('time')).toBe(
      '2026-08-14T05:00:00.000Z..2026-08-14T06:00:00.000Z',
    );
    expect(link.closest('[data-radix-popper-content-wrapper]')?.textContent).toContain(
      'Open current trace',
    );
  });

  it('runs its own action on a click and keeps the menu on the context menu', async () => {
    await i18n.changeLanguage('en-us');
    const onActivate = vi.fn();
    render(
      <MemoryRouter>
        <SignalReference type="trace_id" value="trace-1" onActivate={onActivate} />
      </MemoryRouter>,
    );
    const reference = screen.getByRole('button', { name: /trace-1/ });

    fireEvent.click(reference);
    expect(onActivate).toHaveBeenCalledWith(reference);
    expect(screen.queryByRole('link', { name: /Open current trace/ })).toBeNull();

    fireEvent.contextMenu(reference);
    expect(await screen.findByRole('link', { name: /Open current trace/ })).toBeInTheDocument();
    expect(onActivate).toHaveBeenCalledTimes(1);
  });
});

describe('SignalReference menus', () => {
  function renderTwoReferences(onContainerClick = vi.fn()) {
    render(
      <MemoryRouter>
        {/* A row that acts on a click, as the waterfall's does: a reference's click is not its own. */}
        <div
          data-testid="row"
          onClick={(event) => {
            if (!isNestedControlClick(event)) onContainerClick();
          }}
        >
          <SignalReference type="span_id" value="span-a" labels={{ operation: 'GET /a' }} />
          <SignalReference type="service" value="checkout" />
          <span data-testid="row-text">a span</span>
        </div>
      </MemoryRouter>,
    );
    return {
      spanReference: screen.getByRole('button', { name: /GET \/a|span-a/ }),
      serviceReference: screen.getByRole('button', { name: /checkout/ }),
      onContainerClick,
    };
  }

  const menus = () => screen.queryAllByRole('dialog');

  it('keeps one menu open: a click on another reference closes the one open before', async () => {
    await i18n.changeLanguage('en-us');
    const user = userEvent.setup();
    const { spanReference, serviceReference, onContainerClick } = renderTwoReferences();

    await user.click(spanReference);
    expect(menus()).toHaveLength(1);
    expect(menus()[0]).toHaveTextContent('span-a');

    await user.click(serviceReference);
    expect(menus()).toHaveLength(1);
    expect(menus()[0]).toHaveTextContent('checkout');
    expect(menus()[0]).not.toHaveTextContent('span-a');

    // Neither click was the row's.
    expect(onContainerClick).not.toHaveBeenCalled();
    await user.click(screen.getByTestId('row-text'));
    expect(onContainerClick).toHaveBeenCalledTimes(1);
    expect(menus()).toHaveLength(0);
  });

  it('closes the open menu on a right click on another reference, and the last on Escape', async () => {
    await i18n.changeLanguage('en-us');
    const user = userEvent.setup();
    const { spanReference, serviceReference } = renderTwoReferences();

    await user.pointer({ keys: '[MouseRight]', target: spanReference });
    expect(menus()).toHaveLength(1);
    expect(menus()[0]).toHaveTextContent('span-a');

    // Opened from the context menu, it still closes on a plain click on another reference.
    await user.click(serviceReference);
    expect(menus()).toHaveLength(1);
    expect(menus()[0]).toHaveTextContent('checkout');

    await user.pointer({ keys: '[MouseRight]', target: spanReference });
    expect(menus()).toHaveLength(1);
    expect(menus()[0]).toHaveTextContent('span-a');

    await user.keyboard('{Escape}');
    expect(menus()).toHaveLength(0);
    expect(spanReference).toHaveFocus();
  });

  it('lets its click reach the page, where an open menu learns it was pressed outside', async () => {
    await i18n.changeLanguage('en-us');
    const user = userEvent.setup();
    const onPageClick = vi.fn();
    document.addEventListener('click', onPageClick);
    try {
      render(
        <MemoryRouter>
          <SignalReference type="service" value="checkout" />
        </MemoryRouter>,
      );

      await user.click(screen.getByRole('button', { name: /checkout/ }));
      expect(menus()).toHaveLength(1);
      expect(onPageClick).toHaveBeenCalledTimes(1);
    } finally {
      document.removeEventListener('click', onPageClick);
    }
  });
});
