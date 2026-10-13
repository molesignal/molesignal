import { useQuery, useQueryClient } from '@tanstack/react-query';
import * as React from 'react';

import { deleteUiPreference, getUiPreference, putUiPreference } from '@/api/me';
import { useAuthStore } from '@/stores/auth';

import {
  clearCachedUiPreference,
  readCachedUiPreference,
  type UiPreferenceAddress,
  writeCachedUiPreference,
} from './cache';

/** How long a change waits for the next one before it is sent: a drag settles in less. */
const SAVE_AFTER_MS = 600;

export interface UiPreference<T> {
  /** What is kept: this browser's copy at once, the server's as soon as it has answered. */
  value: T | null;
  /** The server has said what it keeps, or that it cannot. */
  settled: boolean;
  /** Keeps `value` from now on, here at once and on the server a moment later. */
  save: (value: T) => void;
  /** Keeps nothing any more: the page goes back to what it shows by default. */
  clear: () => void;
}

/**
 * How the signed-in user prefers a page to look, by purpose and key: the
 * columns of a log stream, say, by the stream's name.
 *
 * The server's copy is the one that counts, since it is the one another
 * browser sees. Where the server keeps nothing, or does not answer, this
 * browser's copy stands. `parse` reads a stored value and answers `null` for
 * one it does not know, which is then treated as nothing kept.
 *
 * With no `key` there is nothing to keep: the value is `null` and saving does
 * nothing.
 */
export function useUiPreference<T>(
  scope: string,
  key: string | null,
  parse: (raw: unknown) => T | null,
): UiPreference<T> {
  const userId = useAuthStore((state) => state.ctx?.user_id ?? '');
  const orgId = useAuthStore((state) => state.ctx?.org_id ?? '');
  const queryClient = useQueryClient();
  const parseRef = React.useRef(parse);
  parseRef.current = parse;

  const address = React.useMemo<UiPreferenceAddress | null>(
    () => (key && userId && orgId ? { userId, orgId, scope, key } : null),
    [key, orgId, scope, userId],
  );
  const queryKey = React.useMemo(() => ['me', 'ui-preference', userId, orgId, scope, key], [key, orgId, scope, userId]);

  const kept = useQuery({
    queryKey,
    enabled: address !== null,
    // Asked once per page: what is written afterwards is put into this answer by `save`.
    staleTime: Number.POSITIVE_INFINITY,
    retry: false,
    queryFn: async () => {
      const answer = await getUiPreference(scope, key ?? '');

      return answer.value === null || answer.value === undefined ? null : parseRef.current(answer.value);
    },
  });

  // Read again whenever the server's copy changes: `save` and `clear` write both.
  const cached = React.useMemo(
    () => (address ? readCachedUiPreference(address, parseRef.current)?.value ?? null : null),
    // eslint-disable-next-line react-hooks/exhaustive-deps -- `kept.dataUpdatedAt` stands for a write to either copy.
    [address, kept.dataUpdatedAt],
  );

  const pendingRef = React.useRef<{ address: UiPreferenceAddress; value: T; timer: number } | null>(null);
  const flush = React.useCallback(() => {
    const pending = pendingRef.current;
    if (!pending) return;

    window.clearTimeout(pending.timer);
    pendingRef.current = null;
    // A save that fails leaves this browser's copy, which the next save sends again.
    void putUiPreference(pending.address.scope, pending.address.key, pending.value).catch(() => undefined);
  }, []);

  // A change made a moment before leaving the page, or before the key changes, is still sent.
  React.useEffect(() => flush, [address, flush]);

  const save = React.useCallback(
    (value: T) => {
      if (!address) return;

      writeCachedUiPreference(address, value);
      queryClient.setQueryData(queryKey, value);

      if (pendingRef.current) window.clearTimeout(pendingRef.current.timer);
      pendingRef.current = { address, value, timer: window.setTimeout(flush, SAVE_AFTER_MS) };
    },
    [address, flush, queryClient, queryKey],
  );

  const clear = React.useCallback(() => {
    if (!address) return;

    if (pendingRef.current) window.clearTimeout(pendingRef.current.timer);
    pendingRef.current = null;
    clearCachedUiPreference(address);
    queryClient.setQueryData(queryKey, null);
    void deleteUiPreference(address.scope, address.key).catch(() => undefined);
  }, [address, queryClient, queryKey]);

  const settled = address === null || kept.isSuccess || kept.isError;
  // The server's word where it has one; otherwise what this browser remembers.
  const value = address === null ? null : (kept.data ?? cached);

  return React.useMemo(() => ({ value, settled, save, clear }), [clear, save, settled, value]);
}
