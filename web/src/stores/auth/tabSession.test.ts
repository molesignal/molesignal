import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { FakeNetwork } from './__tests__/fakeBroadcast';
import type { AuthContext } from './index';
import { createTabSessionSharing, type SharedSession } from './tabSession';

const ctx = (userId: string): AuthContext => ({
  user_id: userId,
  org_id: 'org-1',
  display_role: 'Owner',
  roles: [{ id: 'role-owner', key: 'owner', name: 'Owner', builtin: true }],
});
const session = (userId: string): SharedSession => ({ token: `token-${userId}`, ctx: ctx(userId) });

let network: FakeNetwork;
const tab = (now: () => number = () => 0) =>
  createTabSessionSharing({ openChannel: () => network.open(), now });

/** Settles a borrow: the answers, the settle window and the give-up timer all pass. */
const settle = () => vi.advanceTimersByTimeAsync(1_000);

beforeEach(() => {
  vi.useFakeTimers();
  network = new FakeNetwork();
});

afterEach(() => {
  vi.useRealTimers();
});

describe('borrowing a session from an open tab', () => {
  it('hands the session of an open tab to a new one', async () => {
    tab().lendSession({ getSession: () => session('a'), onSignOut: vi.fn() });

    const borrowed = tab().borrowSession();
    await settle();

    await expect(borrowed).resolves.toEqual(session('a'));
  });

  it('takes the session of the tab the user was in last', async () => {
    let clockA = 100;
    let clockB = 200;
    tab(() => clockA).lendSession({ getSession: () => session('a'), onSignOut: vi.fn() });
    tab(() => clockB).lendSession({ getSession: () => session('b'), onSignOut: vi.fn() });

    const borrowed = tab().borrowSession();
    await settle();
    await expect(borrowed).resolves.toEqual(session('b'));

    // Using tab a afterwards makes it the most recent one.
    clockA = 300;
    window.dispatchEvent(new Event('pointerdown'));
    const again = tab().borrowSession();
    await settle();
    await expect(again).resolves.toEqual(session('a'));
  });

  it('gives up when no tab answers', async () => {
    const borrowed = tab().borrowSession();
    const settled = vi.fn();
    void borrowed.then(settled);

    await vi.advanceTimersByTimeAsync(299);
    expect(settled).not.toHaveBeenCalled();

    await vi.advanceTimersByTimeAsync(1);
    await expect(borrowed).resolves.toBeNull();
  });

  it('is not answered by a tab that is not signed in', async () => {
    tab().lendSession({ getSession: () => null, onSignOut: vi.fn() });

    const borrowed = tab().borrowSession();
    await settle();

    await expect(borrowed).resolves.toBeNull();
  });

  it('is not answered by a tab that stopped lending', async () => {
    const stop = tab().lendSession({ getSession: () => session('a'), onSignOut: vi.fn() });
    stop();

    const borrowed = tab().borrowSession();
    await settle();

    await expect(borrowed).resolves.toBeNull();
  });

  it('ignores answers that are malformed or meant for another request', async () => {
    const stranger = network.open();
    // The stranger answers every request with nonsense and with a good offer
    // that belongs to a different request.
    stranger.addEventListener('message', (event) => {
      const { id } = event.data as { id: string };
      stranger.postMessage({ type: 'offer', id, session: { token: '', ctx: ctx('x') }, activeAt: 1 });
      stranger.postMessage({ type: 'offer', id, session: { token: 'token-x' }, activeAt: 1 });
      stranger.postMessage({ type: 'offer', id, session: session('x') });
      stranger.postMessage({ type: 'offer', id: 'someone-else', session: session('x'), activeAt: 1 });
      stranger.postMessage('offer');
      stranger.postMessage(null);
    });

    const borrowed = tab().borrowSession();
    await settle();

    await expect(borrowed).resolves.toBeNull();
  });
});

describe('signing out of every tab', () => {
  it('reaches the other tabs but not the one that signed out', async () => {
    const onSignOutA = vi.fn();
    const onSignOutB = vi.fn();
    const onSignOutC = vi.fn();
    const a = tab();
    const b = tab();
    const c = tab();
    a.lendSession({ getSession: () => session('a'), onSignOut: onSignOutA });
    b.lendSession({ getSession: () => session('a'), onSignOut: onSignOutB });
    c.lendSession({ getSession: () => session('a'), onSignOut: onSignOutC });

    b.announceSignOut();
    await settle();

    expect(onSignOutA).toHaveBeenCalledTimes(1);
    expect(onSignOutC).toHaveBeenCalledTimes(1);
    expect(onSignOutB).not.toHaveBeenCalled();
  });

  it('is not heard by a tab that stopped listening', async () => {
    const onSignOut = vi.fn();
    const stop = tab().lendSession({ getSession: () => session('a'), onSignOut });
    stop();

    tab().announceSignOut();
    await settle();

    expect(onSignOut).not.toHaveBeenCalled();
  });
});

describe('where BroadcastChannel does not exist', () => {
  const unsupported = () => createTabSessionSharing({ openChannel: () => null });

  it('borrows nothing, at once', async () => {
    await expect(unsupported().borrowSession()).resolves.toBeNull();
  });

  it('lends and announces nothing, without failing', () => {
    const sharing = unsupported();
    const stop = sharing.lendSession({ getSession: () => session('a'), onSignOut: vi.fn() });

    expect(() => sharing.announceSignOut()).not.toThrow();
    expect(() => stop()).not.toThrow();
  });
});
