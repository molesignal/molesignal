import { act, renderHook } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { PublicSsoProvider } from '@/api/sso';
import {
  keepLastSignin,
  keepSsoTrip,
  readLastSignin,
  takeSsoTrip,
} from '@/shell/signin/lastSignin';
import {
  type ChannelRow,
  channelRows,
  rememberedEmail,
  useChannelsKnown,
  useSigninSteps,
} from '@/shell/signin/useSigninSteps';

const OKTA: PublicSsoProvider = { id: 'okta', name: 'Okta', kind: 'oidc', platform: 'okta' };
const LDAP: PublicSsoProvider = { id: 'corp', name: 'Corp', kind: 'ldap', platform: 'custom' };
const KEY = 'molesignal.signin.last';

describe('what the browser keeps of the last sign-in', () => {
  beforeEach(() => {
    window.localStorage.clear();
    window.sessionStorage.clear();
  });

  it('keeps the way in and the account, and nothing else', () => {
    keepLastSignin({ channel: { kind: 'password' }, account: 'alice@example.com' });

    expect(window.localStorage.getItem(KEY)).toBe(
      '{"channel":{"kind":"password"},"account":"alice@example.com"}',
    );
    expect(readLastSignin()).toEqual({
      channel: { kind: 'password' },
      account: 'alice@example.com',
    });
  });

  it('takes the account from the sign-in when the way in does not say', () => {
    keepLastSignin({ channel: { kind: 'provider', id: 'okta' } }, 'alice@example.com');
    expect(readLastSignin()).toEqual({
      channel: { kind: 'provider', id: 'okta' },
      account: 'alice@example.com',
    });

    // A directory's username is what its form asks for, not the email it turned out to have.
    keepLastSignin(
      { channel: { kind: 'provider', id: 'corp' }, account: 'alice' },
      'alice@example.com',
    );
    expect(readLastSignin()?.account).toBe('alice');

    keepLastSignin({ channel: { kind: 'passkey' } });
    expect(readLastSignin()).toEqual({ channel: { kind: 'passkey' } });
  });

  it.each([
    ['nothing', null],
    ['broken JSON', '{'],
    ['a stray value', '"password"'],
    ['an unknown way in', '{"channel":{"kind":"magic"}}'],
    ['a provider without an id', '{"channel":{"kind":"provider"}}'],
  ])('reads %s as nothing to point at', (_name, stored) => {
    if (stored !== null) window.localStorage.setItem(KEY, stored);

    expect(readLastSignin()).toBeNull();
  });

  it('ignores an account that is not text', () => {
    window.localStorage.setItem(KEY, '{"channel":{"kind":"passkey"},"account":42}');

    expect(readLastSignin()).toEqual({ channel: { kind: 'passkey' } });
  });

  it('carries on without storage', () => {
    const getItem = vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => {
      throw new Error('blocked');
    });
    const setItem = vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
      throw new Error('blocked');
    });

    expect(readLastSignin()).toBeNull();
    expect(() => keepLastSignin({ channel: { kind: 'passkey' } })).not.toThrow();
    expect(() => keepSsoTrip(true, 'okta')).not.toThrow();
    expect(takeSsoTrip()).toEqual({ remember: false, providerId: null });

    getItem.mockRestore();
    setItem.mockRestore();
  });

  it('carries "remember me" and the provider across the trip, once', () => {
    keepSsoTrip(true, 'okta');
    expect(takeSsoTrip()).toEqual({ remember: true, providerId: 'okta' });
    expect(takeSsoTrip()).toEqual({ remember: false, providerId: null });
  });
});

describe('the ways in the sign-in page lists', () => {
  it('lists the passkey and the providers, none marked on a first visit', () => {
    expect(channelRows([OKTA], true, null)).toEqual([
      { kind: 'passkey', lastUsed: false },
      { kind: 'provider', provider: OKTA, lastUsed: false },
    ]);
    expect(channelRows([], false, null)).toEqual([]);
  });

  it('puts the account that last used its password first', () => {
    const rows = channelRows([OKTA], false, {
      channel: { kind: 'password' },
      account: 'alice@example.com',
    });

    expect(rows[0]).toEqual({ kind: 'account', account: 'alice@example.com', lastUsed: true });
    expect(rows[1]).toEqual({ kind: 'provider', provider: OKTA, lastUsed: false });
  });

  it('marks the provider used last and says whose account it was', () => {
    const rows = channelRows([OKTA, LDAP], true, {
      channel: { kind: 'provider', id: 'corp' },
      account: 'alice',
    });

    expect(rows).toEqual([
      { kind: 'passkey', lastUsed: false },
      { kind: 'provider', provider: OKTA, lastUsed: false },
      { kind: 'provider', provider: LDAP, lastUsed: true, account: 'alice' },
    ]);
  });

  it('marks nothing when the way in used last is gone', () => {
    const rows = channelRows([OKTA], false, {
      channel: { kind: 'provider', id: 'removed' },
      account: 'alice@example.com',
    });

    expect(rows).toEqual([{ kind: 'provider', provider: OKTA, lastUsed: false }]);
  });

  it('fills in an email, not a directory username', () => {
    expect(
      rememberedEmail({ channel: { kind: 'passkey' }, account: 'alice@example.com' }),
    ).toBe('alice@example.com');
    expect(rememberedEmail({ channel: { kind: 'provider', id: 'corp' }, account: 'alice' })).toBe('');
    expect(rememberedEmail(null)).toBe('');
  });
});

describe('the steps of the sign-in', () => {
  const ROWS: ChannelRow[] = [{ kind: 'provider', provider: OKTA, lastUsed: false }];

  it('goes one question at a time and back the same way', () => {
    const { result } = renderHook(() => useSigninSteps(ROWS, true));

    expect(result.current.step).toEqual({ name: 'channels' });
    expect(result.current.canGoBack).toBe(false);

    act(() => result.current.askIdentifier(null));
    expect(result.current.step).toEqual({ name: 'identifier', directory: null });
    expect(result.current.canGoBack).toBe(true);

    act(() => result.current.askPassword(null));
    expect(result.current.step).toEqual({ name: 'password', directory: null });

    act(() => result.current.back());
    expect(result.current.step.name).toBe('identifier');

    act(() => result.current.back());
    expect(result.current.step.name).toBe('channels');
  });

  it('starts with the email when there is nothing to list', () => {
    const { result } = renderHook(() => useSigninSteps([], true));

    expect(result.current.step).toEqual({ name: 'identifier', directory: null });
    expect(result.current.canGoBack).toBe(false);

    act(() => result.current.askPassword(null));
    expect(result.current.step.name).toBe('password');
    expect(result.current.canGoBack).toBe(true);

    act(() => result.current.back());
    expect(result.current.step.name).toBe('identifier');
    expect(result.current.canGoBack).toBe(false);
  });

  it('still leads to the question when a known account skipped it', () => {
    const { result } = renderHook(() => useSigninSteps(ROWS, true));

    act(() => result.current.skipToPassword(LDAP));
    expect(result.current.step).toEqual({ name: 'password', directory: LDAP });

    act(() => result.current.back());
    expect(result.current.step).toEqual({ name: 'identifier', directory: LDAP });
  });

  it('goes back to the ways in when a workspace lets only its providers in', () => {
    const { result } = renderHook(() => useSigninSteps(ROWS, true));

    act(() => result.current.askIdentifier(null));
    act(() => result.current.askPassword(null));
    act(() => result.current.restart());

    expect(result.current.step).toEqual({ name: 'channels' });
    expect(result.current.canGoBack).toBe(false);
  });

  it('waits for the list, then lets a late answer change nothing once someone is typing', () => {
    const { result, rerender } = renderHook(
      ({ rows, known }: { rows: ChannelRow[]; known: boolean }) => useSigninSteps(rows, known),
      { initialProps: { rows: [] as ChannelRow[], known: false } },
    );

    expect(result.current.step.name).toBe('waiting');

    rerender({ rows: [], known: true });
    expect(result.current.step.name).toBe('identifier');

    act(() => result.current.stayOnEmail());
    rerender({ rows: ROWS, known: true });
    expect(result.current.step.name).toBe('identifier');
    // What arrived late can still be reached.
    expect(result.current.canGoBack).toBe(true);
  });

  it('shows a list that arrives late to someone who has not started typing', () => {
    const { result, rerender } = renderHook(
      ({ rows }: { rows: ChannelRow[] }) => useSigninSteps(rows, true),
      { initialProps: { rows: [] as ChannelRow[] } },
    );

    expect(result.current.step.name).toBe('identifier');

    rerender({ rows: ROWS });
    expect(result.current.step.name).toBe('channels');
  });
});

describe('how long the page waits for the list', () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it('stops waiting when the answer arrives, or after a moment without one', () => {
    vi.useFakeTimers();

    const { result, rerender } = renderHook(({ loading }) => useChannelsKnown(loading), {
      initialProps: { loading: true },
    });

    expect(result.current).toBe(false);

    act(() => {
      vi.advanceTimersByTime(1_000);
    });
    expect(result.current).toBe(true);

    rerender({ loading: false });
    expect(result.current).toBe(true);
  });
});
