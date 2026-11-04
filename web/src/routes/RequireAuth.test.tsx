import { act, cleanup, render, screen } from '@testing-library/react';
import * as React from 'react';
import { MemoryRouter, Route, Routes, useLocation } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type * as HttpModule from '@/lib/http';
import type { AuthContext } from '@/stores/auth';
import { FakeNetwork } from '@/stores/auth/__tests__/fakeBroadcast';

const ctx = (userId: string, orgId = 'org-1'): AuthContext => ({
  userId: userId,
  orgId: orgId,
  displayRole: 'Owner',
  roles: [{ id: 'role-owner', key: 'owner', name: 'Owner', builtin: true }],
});

let network: FakeNetwork;

/**
 * The gate keeps per-page state (it only asks once), so every test gets a page
 * of its own: fresh modules, a BroadcastChannel wired to a fake browser, and a
 * server whose answer to "which session does the cookie hold" is `cookie`.
 */
async function loadPage(cookie: AuthContext | null = null) {
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
  const currentSession = vi.fn(async () => cookie);
  const adoptCurrentSession = vi.fn(async () => {});
  vi.doMock('@/lib/http', async (importOriginal) => ({
    ...(await importOriginal<typeof HttpModule>()),
    currentSession,
    adoptCurrentSession,
  }));
  const [{ RequireAuth }, { useAuthStore }, { createTabSessionSharing }, { useOrgStore }] =
    await Promise.all([
      import('./RequireAuth'),
      import('@/stores/auth'),
      import('@/stores/auth/tabSession'),
      import('@/stores/useOrgStore'),
    ]);
  /** Another tab of the same browser. */
  const otherTab = () => createTabSessionSharing({ openChannel: () => network.open() });
  return {
    RequireAuth,
    useAuthStore,
    useOrgStore,
    otherTab,
    currentSession,
    adoptCurrentSession,
  };
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

/** Lets the server answer and the gate settle. */
const wait = () => act(() => vi.advanceTimersByTimeAsync(10));

beforeEach(() => {
  vi.useFakeTimers();
  localStorage.clear();
  sessionStorage.clear();
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.unstubAllGlobals();
  vi.doUnmock('@/lib/http');
});

describe('RequireAuth', () => {
  it('shows the app to a tab that is already signed in, without asking the server', async () => {
    const { RequireAuth, useAuthStore, currentSession } = await loadPage();
    useAuthStore.setState({ ctx: ctx('me') });

    renderGate(RequireAuth);
    await wait();

    expect(screen.getByText('the app')).toBeInTheDocument();
    expect(currentSession).not.toHaveBeenCalled();
  });

  it('opens the browser session in a tab opened by hand instead of asking to sign in', async () => {
    const { RequireAuth, useAuthStore } = await loadPage(ctx('a'));

    renderGate(RequireAuth);
    // Neither the app nor the sign-in page while the server is asked.
    expect(screen.queryByText('the app')).toBeNull();
    expect(screen.queryByText('sign-in page')).toBeNull();
    await wait();

    expect(screen.getByText('the app')).toBeInTheDocument();
    expect(useAuthStore.getState().ctx?.userId).toBe('a');
  });

  it('sends the tab to sign in, remembering where it was, when the browser has no session', async () => {
    const { RequireAuth } = await loadPage(null);

    renderGate(RequireAuth);
    await wait();

    expect(screen.getByText('sign-in page')).toBeInTheDocument();
    expect(screen.getByTestId('where').textContent).toBe(
      '/signin?next=%2Flogs%3Fstream%3Dapp',
    );
  });

  it('does not ask again after the tab signed out', async () => {
    const { RequireAuth, useAuthStore, currentSession } = await loadPage(ctx('a'));
    renderGate(RequireAuth);
    await wait();
    expect(screen.getByText('the app')).toBeInTheDocument();

    act(() => useAuthStore.getState().logout());
    await wait();

    expect(screen.getByText('sign-in page')).toBeInTheDocument();
    expect(currentSession).toHaveBeenCalledTimes(1);
  });

  it('does not ask when the gate mounts again after the tab signed out', async () => {
    const { RequireAuth, useAuthStore, currentSession } = await loadPage(ctx('a'));
    renderGate(RequireAuth);
    await wait();
    act(() => useAuthStore.getState().logout());
    cleanup();

    renderGate(RequireAuth);

    expect(screen.getByText('sign-in page')).toBeInTheDocument();
    await wait();
    expect(currentSession).toHaveBeenCalledTimes(1);
    expect(useAuthStore.getState().ctx).toBeNull();
  });

  it('asks once even when React runs its effects twice', async () => {
    const { RequireAuth, currentSession } = await loadPage(ctx('a'));

    renderGate(RequireAuth, { strict: true });
    await wait();

    expect(screen.getByText('the app')).toBeInTheDocument();
    expect(currentSession).toHaveBeenCalledTimes(1);
  });

  it('signs out here, and starts over on a fresh page, when another tab signs out on purpose', async () => {
    const assign = vi.fn();
    vi.stubGlobal('location', { pathname: '/logs', search: '?stream=app', assign });
    const { RequireAuth, useAuthStore, useOrgStore, otherTab } = await loadPage();
    useAuthStore.setState({ ctx: ctx('me') });
    useOrgStore.setState({ loaded: true });
    renderGate(RequireAuth);

    otherTab().announceSignOut();
    await wait();

    expect(useAuthStore.getState().ctx).toBeNull();
    expect(useOrgStore.getState().loaded).toBe(false);
    expect(assign).toHaveBeenCalledWith('/signin?next=%2Flogs%3Fstream%3Dapp');
  });

  it('follows another tab into the session it switched to', async () => {
    const { RequireAuth, useAuthStore, otherTab, adoptCurrentSession } = await loadPage();
    useAuthStore.setState({ ctx: ctx('me') });
    renderGate(RequireAuth);

    otherTab().announceSession('me:org-1');
    await wait();
    expect(adoptCurrentSession).not.toHaveBeenCalled();

    otherTab().announceSession('me:org-2');
    await wait();
    expect(adoptCurrentSession).toHaveBeenCalledTimes(1);
  });
});
