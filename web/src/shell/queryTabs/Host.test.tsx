import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import * as React from 'react';
import { createMemoryRouter, Link, RouterProvider } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import i18n from '@/i18n';
import { TooltipProvider } from '@/shell/ui/tooltip';
import { useAuthStore } from '@/stores/auth';
import { DEFAULT_WINDOW, type TimeWindow, useTimeStore } from '@/stores/useTimeStore';

import { QueryTabs } from './Host';
import {
  useQueryTabSearchParams,
  useQueryTabState,
  useQueryTabStatus,
} from './scope';
import { QUERY_TABS_STORAGE_KEY, useQueryTabsRuntime, useQueryTabsStore } from './store';

const SIX_HOURS: TimeWindow = { from: 'now-6h', to: 'now', mode: 'relative' };

const COPY = {
  setQuery: 'Query errors',
  setRange: 'Last 6 hours',
  count: 'Count a result',
  run: 'Run',
  finish: 'Finish',
  note: 'Note',
  home: 'Elsewhere',
  back: 'Back to the explorer',
} as const;

/**
 * A stand-in for an explorer: it reads its query from the search params, keeps
 * results in component state and shows the time range it is on.
 */
function Explorer() {
  const [params, setParams] = useQueryTabSearchParams();
  const window = useTimeStore((state) => state.window);
  const setWindow = useTimeStore((state) => state.setWindow);
  const [results, setResults] = React.useState(0);
  const [running, setRunning] = React.useState(false);
  const [note, setNote] = useQueryTabState('note', '');
  useQueryTabStatus({ running, summary: params.get('q') ?? '' });

  return (
    <section data-testid="explorer">
      <output data-testid="query">{params.get('q') ?? '(none)'}</output>
      <output data-testid="range">{`${window.from}..${window.to}`}</output>
      <output data-testid="results">{results}</output>
      <output data-testid="has-time-param">{String(params.has('time'))}</output>
      <button
        type="button"
        onClick={() =>
          setParams(
            (current) => {
              const next = new URLSearchParams(current);
              next.set('q', 'errors');
              return next;
            },
            { replace: true },
          )
        }
      >
        {COPY.setQuery}
      </button>
      <button type="button" onClick={() => setWindow(SIX_HOURS)}>
        {COPY.setRange}
      </button>
      <button type="button" onClick={() => setResults((count) => count + 1)}>
        {COPY.count}
      </button>
      <button type="button" onClick={() => setRunning(true)}>
        {COPY.run}
      </button>
      <button type="button" onClick={() => setRunning(false)}>
        {COPY.finish}
      </button>
      <input aria-label={COPY.note} value={note} onChange={(event) => setNote(event.target.value)} />
    </section>
  );
}

function renderExplorer(initialEntry = '/metrics') {
  const router = createMemoryRouter(
    [
      {
        path: '/metrics',
        element: (
          <QueryTabs kind="metrics">
            <Explorer />
          </QueryTabs>
        ),
      },
      { path: '/elsewhere', element: <Link to="/metrics?q=from-link">{COPY.back}</Link> },
    ],
    { initialEntries: [initialEntry] },
  );
  const view = render(
    <TooltipProvider>
      <RouterProvider router={router} />
    </TooltipProvider>,
  );
  return { router, ...view };
}

/** The explorer in front; the ones behind stay mounted but are not shown. */
function front() {
  const panel = document.querySelector<HTMLElement>('[data-query-tab-panel][data-active="true"]');
  if (!panel) throw new Error('no query tab is in front');
  return within(panel);
}

function tabs() {
  return screen.getAllByRole('tab');
}

function addTab() {
  fireEvent.click(screen.getByRole('button', { name: 'New tab' }));
}

beforeEach(async () => {
  await i18n.changeLanguage('en-us');
  localStorage.removeItem(QUERY_TABS_STORAGE_KEY);
  useQueryTabsStore.setState({ scopes: {} });
  useQueryTabsRuntime.setState({ running: {} });
  useTimeStore.setState({ window: DEFAULT_WINDOW, anchor: null });
  useAuthStore.setState({
    ctx: { user_id: 'u1', org_id: 'acme', display_role: 'admin', roles: [] },
  });
});

afterEach(() => {
  cleanup();
  useAuthStore.setState({ ctx: null });
  useTimeStore.setState({ window: DEFAULT_WINDOW, anchor: null });
});

describe('QueryTabs', () => {
  it('opens on one tab that holds the query in the address', async () => {
    renderExplorer('/metrics?q=latency');

    expect(tabs()).toHaveLength(1);
    expect(front().getByTestId('query').textContent).toBe('latency');
    // The page said what it is querying, so the tab is named after it.
    await waitFor(() => expect(tabs()[0]?.textContent).toContain('latency'));
  });

  it('gives each tab its own query: writing params in one leaves the other alone', async () => {
    const { router } = renderExplorer('/metrics?q=latency');
    addTab();

    expect(tabs()).toHaveLength(2);
    expect(front().getByTestId('query').textContent).toBe('(none)');
    await waitFor(() => expect(router.state.location.search).toBe(''));

    fireEvent.click(front().getByRole('button', { name: COPY.setQuery }));
    expect(front().getByTestId('query').textContent).toBe('errors');
    await waitFor(() => expect(router.state.location.search).toBe('?q=errors'));

    fireEvent.click(tabs()[0]!);
    expect(front().getByTestId('query').textContent).toBe('latency');
    // The address bar follows the tab in front.
    await waitFor(() => expect(router.state.location.search).toBe('?q=latency'));
  });

  it('keeps a tab mounted behind the others, so its results are there when it comes back', () => {
    renderExplorer();
    fireEvent.click(front().getByRole('button', { name: COPY.count }));
    fireEvent.click(front().getByRole('button', { name: COPY.count }));
    addTab();
    expect(front().getByTestId('results').textContent).toBe('0');

    fireEvent.click(tabs()[0]!);

    expect(front().getByTestId('results').textContent).toBe('2');
    expect(document.querySelectorAll('[data-query-tab-panel]')).toHaveLength(2);
  });

  it('gives each tab its own time range and keeps the app on the range of the tab in front', () => {
    renderExplorer();
    addTab();

    fireEvent.click(front().getByRole('button', { name: COPY.setRange }));
    expect(front().getByTestId('range').textContent).toBe('now-6h..now');
    expect(useTimeStore.getState().window).toEqual(SIX_HOURS);
    // The page reads its range from its params the way a URL carries it.
    expect(front().getByTestId('has-time-param').textContent).toBe('true');

    fireEvent.click(tabs()[0]!);
    expect(front().getByTestId('range').textContent).toBe('now-1h..now');
    expect(useTimeStore.getState().window).toEqual(DEFAULT_WINDOW);

    fireEvent.click(tabs()[1]!);
    expect(front().getByTestId('range').textContent).toBe('now-6h..now');
    expect(useTimeStore.getState().window).toEqual(SIX_HOURS);
  });

  it('applies a range set outside the tabs to the one in front only', () => {
    renderExplorer();
    addTab();

    act(() => useTimeStore.getState().setWindow(SIX_HOURS));

    expect(front().getByTestId('range').textContent).toBe('now-6h..now');
    fireEvent.click(tabs()[0]!);
    expect(front().getByTestId('range').textContent).toBe('now-1h..now');
  });

  it('shows which tab is running a query, wherever that tab is', () => {
    renderExplorer();
    fireEvent.click(front().getByRole('button', { name: COPY.run }));
    addTab();

    const [first, second] = document.querySelectorAll('[data-query-tab]');
    expect(first?.querySelector('[data-query-tab-running]')).not.toBeNull();
    expect(second?.querySelector('[data-query-tab-running]')).toBeNull();
  });

  it('takes a link that arrives on the page into the tab in front', async () => {
    const { router } = renderExplorer('/metrics?q=latency');

    await act(() => router.navigate('/metrics?q=from-palette'));

    expect(tabs()).toHaveLength(1);
    await waitFor(() => expect(front().getByTestId('query').textContent).toBe('from-palette'));
  });

  it('closes a tab and brings its neighbour forward', async () => {
    const { router } = renderExplorer('/metrics?q=latency');
    addTab();
    await waitFor(() => expect(router.state.location.search).toBe(''));

    fireEvent.click(screen.getAllByRole('button', { name: /^Close / })[1]!);

    expect(tabs()).toHaveLength(1);
    expect(front().getByTestId('query').textContent).toBe('latency');
    await waitFor(() => expect(router.state.location.search).toBe('?q=latency'));
    // The last tab cannot be closed away.
    expect(screen.queryByRole('button', { name: /^Close / })).toBeNull();
  });

  it('brings the tabs back after the page is left, with the query and state each one had', async () => {
    const first = renderExplorer('/metrics?q=latency');
    fireEvent.change(front().getByLabelText(COPY.note), { target: { value: 'checkout' } });
    addTab();
    fireEvent.click(front().getByRole('button', { name: COPY.setQuery }));
    await waitFor(() => expect(first.router.state.location.search).toBe('?q=errors'));
    first.unmount();

    renderExplorer('/metrics');

    expect(tabs()).toHaveLength(2);
    // The tab that was in front is in front again; the one behind mounts when asked for.
    expect(front().getByTestId('query').textContent).toBe('errors');
    expect(document.querySelectorAll('[data-query-tab-panel]')).toHaveLength(1);

    fireEvent.click(tabs()[0]!);
    expect(front().getByTestId('query').textContent).toBe('latency');
    expect((front().getByLabelText(COPY.note) as HTMLInputElement).value).toBe('checkout');
  });

  it('opens a link from another page in a new tab instead of over the query in front', async () => {
    const first = renderExplorer('/metrics?q=latency');
    await waitFor(() => expect(tabs()[0]?.textContent).toContain('latency'));
    first.unmount();

    const { router } = renderExplorer('/elsewhere');
    fireEvent.click(screen.getByRole('link', { name: COPY.back }));

    await waitFor(() => expect(tabs()).toHaveLength(2));
    expect(front().getByTestId('query').textContent).toBe('from-link');
    expect(router.state.location.search).toBe('?q=from-link');
  });

  it('keeps the tabs of another organization apart', () => {
    const first = renderExplorer('/metrics?q=latency');
    addTab();
    first.unmount();

    useAuthStore.setState({
      ctx: { user_id: 'u1', org_id: 'other', display_role: 'admin', roles: [] },
    });
    renderExplorer('/metrics');

    expect(tabs()).toHaveLength(1);
    expect(front().getByTestId('query').textContent).toBe('(none)');
  });
});
