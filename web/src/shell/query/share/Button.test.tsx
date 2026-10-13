import '@/i18n';

import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { DEFAULT_USER_PREFERENCES } from '@/api/me';
import { TooltipProvider } from '@/shell/ui/tooltip';
import { useFiltersStore } from '@/stores/useFiltersStore';
import { DEFAULT_WINDOW, useTimeStore, type TimeWindow } from '@/stores/useTimeStore';

const clipboard = vi.hoisted(() => ({ writeClipboardText: vi.fn() }));
vi.mock('@/lib/clipboard', () => clipboard);

import { QueryShareButton } from './Button';

const LAST_15M: TimeWindow = { mode: 'relative', from: 'now-15m', to: 'now' };
const SEARCH = new URLSearchParams({ stream: 'application_logs', q: "level = 'ERROR'" });

function renderShare(window: TimeWindow = LAST_15M) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, staleTime: Infinity } } });
  // The reader's clock is UTC, whatever zone the tests run in.
  client.setQueryData(['me', 'preferences'], { ...DEFAULT_USER_PREFERENCES, timezone: 'UTC' });
  useTimeStore.setState({ window, anchor: null });

  return render(
    <MemoryRouter initialEntries={['/logs?stream=stale']}>
      <QueryClientProvider client={client}>
        <TooltipProvider>
          <QueryShareButton search={SEARCH} />
        </TooltipProvider>
      </QueryClientProvider>
    </MemoryRouter>,
  );
}

/** Opens the panel and gives the link it shows, parsed. */
async function openShare(user: ReturnType<typeof userEvent.setup>) {
  await user.click(screen.getByRole('button', { name: 'Share' }));

  const panel = within(await screen.findByRole('dialog'));
  const link = () => new URL((panel.getByRole('textbox', { name: 'Link to this query' }) as HTMLInputElement).value);

  return { panel, link };
}

beforeEach(() => {
  clipboard.writeClipboardText.mockReset();
  clipboard.writeClipboardText.mockResolvedValue(undefined);
});

afterEach(() => {
  cleanup();
  useTimeStore.setState({ window: DEFAULT_WINDOW, anchor: null });
  useFiltersStore.setState({ filters: [] });
});

describe('QueryShareButton', () => {
  it('gives a link to the query on screen, with the moments on screen fixed in it', async () => {
    const user = userEvent.setup();
    renderShare();
    const { panel, link } = await openShare(user);

    // The page's own address, with the query the page passed and not what the address bar still says.
    expect(link().pathname).toBe('/logs');
    expect(link().searchParams.get('stream')).toBe('application_logs');
    expect(link().searchParams.get('q')).toBe("level = 'ERROR'");

    // "The last 15 minutes" is fixed to the fifteen minutes that ended when the reader asked.
    const [from, to] = (link().searchParams.get('time') ?? '').split('..').map((moment) => Date.parse(moment));
    expect(to! - from!).toBe(15 * 60_000);
    expect(Math.abs(Date.now() - to!)).toBeLessThan(5_000);
    expect(panel.getByRole('radio', { name: /^Current time selection/ }).getAttribute('aria-checked')).toBe('true');
    // The same moments are said in words, on the reader's clock.
    expect(document.querySelector('[data-query-share-moments]')?.textContent).toMatch(
      /^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2} – \d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}$/,
    );
  });

  it('keeps the time moving when asked: the last fifteen minutes of whoever opens the link', async () => {
    const user = userEvent.setup();
    renderShare();
    const { panel, link } = await openShare(user);

    await user.click(panel.getByRole('radio', { name: 'Last 15 minutes, from when it is opened' }));

    expect(link().searchParams.get('time')).toBe('now-15m..now');
  });

  it('copies the link, and says so on the button that did', async () => {
    const user = userEvent.setup();
    renderShare();
    const { panel, link } = await openShare(user);

    await user.click(panel.getByRole('button', { name: 'Copy link' }));

    expect(clipboard.writeClipboardText).toHaveBeenCalledExactlyOnceWith(link().toString());
    await waitFor(() => expect(panel.getByRole('button', { name: 'Link copied' })).not.toBeNull());
  });

  it('has no choice to offer for a time range that is moments already', async () => {
    const user = userEvent.setup();
    renderShare({ mode: 'absolute', from: '2026-10-12T08:00:00.000Z', to: '2026-10-12T09:00:00.000Z' });
    const { panel, link } = await openShare(user);

    expect(panel.queryByRole('radio')).toBeNull();
    expect(link().searchParams.get('time')).toBe('2026-10-12T08:00:00.000Z..2026-10-12T09:00:00.000Z');
    expect(document.querySelector('[data-query-share-moments]')?.textContent).toBe(
      '2026-10-12 08:00:00 – 2026-10-12 09:00:00',
    );
  });

  it('carries the filters pinned across pages, which the query runs under', async () => {
    const user = userEvent.setup();
    useFiltersStore.setState({ filters: [{ key: 'env', value: 'prod', operator: '=' }] });
    renderShare();
    const { link } = await openShare(user);

    expect(link().searchParams.get('filters')).toBe('[["env","=","prod"]]');
  });
});
