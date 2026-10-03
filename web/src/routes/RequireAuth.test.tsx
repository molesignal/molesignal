import { act, cleanup, render, screen } from '@testing-library/react';
import * as React from 'react';
import { MemoryRouter, Route, Routes, useLocation } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { AuthContext } from '@/stores/auth';
import { FakeNetwork } from '@/stores/auth/__tests__/fakeBroadcast';

const ctx = (userId: string): AuthContext => ({
  user_id: userId,
  org_id: 'org-1',
  display_role: 'Owner',
  roles: [{ id: 'role-owner', key: 'owner', name: 'Owner', builtin: true }],
});
const session = (userId: string) => ({ token: `token-${userId}`, ctx: ctx(userId) });

let network: FakeNetwork;

// The gate keeps per-page state (it only asks once), so every test gets a page
// of its own: fresh modules, and a BroadcastChannel wired to a fake browser.
async function loadPage() {
  vi.resetModules();
  network = new FakeNetwork();
  vi.stubGlobal(
    'BroadcastChannel',
    class {
      constructor() {
        return network.open();
      }
    },
  );
  const [{ RequireAuth }, { useAuthStore }, { createTabSessionSharing }, { useOrgStore }] =
    await Promise.all([
      import('./RequireAuth'),
      import('@/stores/auth'),
      import('@/stores/auth/tabSession'),
      import('@/stores/useOrgStore'),
    ]);
  /** Another tab of the same browser. */
  const otherTab = () => createTabSessionSharing({ openChannel: () => network.open() });
  return { RequireAuth, useAuthStore, useOrgStore, otherTab };
}

function Where() {
  const location = useLocation();
  return <p data-testid="where">{`${location.pathname}${location.search}`}</p>;
}

function renderGate(
  RequireAuth: React.ComponentType<{ children: React.ReactNode }>,
  { strict = false }: { strict?: boolean } = {},
) {
  const tree = (
    <MemoryRouter initialEntries={['/logs?stream=app']}>
      <Routes>
        <Route
          path="/signin"
          element={
            <>
              <p>sign-in page</p>
              <Where />
            </>
          }
        />
        <Route
          path="*"
          element={
            <RequireAuth>
              <p>the app</p>
            </RequireAuth>
          }
        />
      </Routes>
    </MemoryRouter>
  );
  return render(strict ? <React.StrictMode>{tree}</React.StrictMode> : tree);
}

/** Lets the other tabs answer and every timer of the gate run out. */
const wait = (ms = 1_000) => act(() => vi.advanceTimersByTimeAsync(ms));

/** Records the session requests that cross the (fake) browser. */
function watchRequests() {
  const requests: unknown[] = [];
  network.open().addEventListener('message', (event) => {
    if ((event.data as { type?: string }).type === 'request') requests.push(event.data);
  });
  return requests;
}

beforeEach(() => {
  vi.useFakeTimers();
  localStorage.clear();
  sessionStorage.clear();
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe('RequireAuth', () => {
  it('shows the app to a tab that is already signed in, without asking anyone', async () => {
    const { RequireAuth, useAuthStore } = await loadPage();
    const requests = watchRequests();
    useAuthStore.setState(session('me'));

    renderGate(RequireAuth);
    await wait();

    expect(screen.getByText('the app')).toBeInTheDocument();
    expect(requests).toHaveLength(0);
  });

  it('takes the session of an open tab instead of sending a new tab to sign in', async () => {
    const { RequireAuth, useAuthStore, otherTab } = await loadPage();
    otherTab().lendSession({ getSession: () => session('a'), onSignOut: vi.fn() });

    renderGate(RequireAuth);
    // Neither the app nor the sign-in page while the other tabs are asked.
    expect(screen.queryByText('the app')).toBeNull();
    expect(screen.queryByText('sign-in page')).toBeNull();
    await wait();

    expect(screen.getByText('the app')).toBeInTheDocument();
    expect(useAuthStore.getState().token).toBe('token-a');
    expect(useAuthStore.getState().ctx?.user_id).toBe('a');
    // Kept for this tab only: it ends with the tab, not with the browser session.
    expect(sessionStorage.getItem('molesignal-auth')).toContain('token-a');
    expect(localStorage.getItem('molesignal-auth')).toBeNull();
  });

  it('sends the tab to sign in, remembering where it was, when no tab answers', async () => {
    const { RequireAuth } = await loadPage();

    renderGate(RequireAuth);
    await wait(299);
    expect(screen.queryByText('sign-in page')).toBeNull();

    await wait(1);
    expect(screen.getByText('sign-in page')).toBeInTheDocument();
    expect(screen.getByTestId('where').textContent).toBe(
      '/signin?next=%2Flogs%3Fstream%3Dapp',
    );
  });

  it('does not ask again after the tab signed out', async () => {
    const { RequireAuth, useAuthStore, otherTab } = await loadPage();
    const requests = watchRequests();
    otherTab().lendSession({ getSession: () => session('a'), onSignOut: vi.fn() });
    renderGate(RequireAuth);
    await wait();
    expect(screen.getByText('the app')).toBeInTheDocument();
    expect(requests).toHaveLength(1);

    act(() => useAuthStore.getState().logout());
    await wait();

    expect(screen.getByText('sign-in page')).toBeInTheDocument();
    expect(requests).toHaveLength(1);
  });

  it('does not ask when the gate mounts again after the tab signed out', async () => {
    const { RequireAuth, useAuthStore, otherTab } = await loadPage();
    const requests = watchRequests();
    otherTab().lendSession({ getSession: () => session('a'), onSignOut: vi.fn() });
    renderGate(RequireAuth);
    await wait();
    act(() => useAuthStore.getState().logout());
    cleanup();

    // Back to a signed-in page by hand: the other tab still has a session to
    // give, but this tab signed out and must not take it back.
    renderGate(RequireAuth);

    expect(screen.getByText('sign-in page')).toBeInTheDocument();
    await wait();
    expect(requests).toHaveLength(1);
    expect(useAuthStore.getState().token).toBeNull();
  });

  it('asks once even when React runs its effects twice', async () => {
    const { RequireAuth, otherTab } = await loadPage();
    const requests = watchRequests();
    otherTab().lendSession({ getSession: () => session('a'), onSignOut: vi.fn() });

    renderGate(RequireAuth, { strict: true });
    await wait();

    expect(screen.getByText('the app')).toBeInTheDocument();
    expect(requests).toHaveLength(1);
  });

  it('lends its own session to tabs opened later, until it signs out', async () => {
    const { RequireAuth, useAuthStore, otherTab } = await loadPage();
    useAuthStore.setState(session('me'));
    renderGate(RequireAuth);

    const first = otherTab().borrowSession();
    await wait();
    await expect(first).resolves.toEqual(session('me'));

    act(() => useAuthStore.getState().logout());
    const second = otherTab().borrowSession();
    await wait();
    await expect(second).resolves.toBeNull();
  });

  it('signs out here, and starts over on a fresh page, when another tab signs out on purpose', async () => {
    const assign = vi.fn();
    vi.stubGlobal('location', { pathname: '/logs', search: '?stream=app', assign });
    const { RequireAuth, useAuthStore, useOrgStore, otherTab } = await loadPage();
    useAuthStore.setState(session('me'));
    useOrgStore.setState({ loaded: true });
    renderGate(RequireAuth);

    otherTab().announceSignOut();
    await wait();

    expect(useAuthStore.getState().token).toBeNull();
    expect(useOrgStore.getState().loaded).toBe(false);
    expect(assign).toHaveBeenCalledWith('/signin?next=%2Flogs%3Fstream%3Dapp');
  });
});
