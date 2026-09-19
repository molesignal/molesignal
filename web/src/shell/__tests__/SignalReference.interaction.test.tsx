import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, describe, expect, it } from 'vitest';

import i18n from '@/i18n';
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
});
