import * as React from 'react';
import { useSearchParams } from 'react-router-dom';

import { useAuthStore } from '@/stores/auth';
import { resolveWindow, useTimeStore } from '@/stores/useTimeStore';

import { type ApmScope, compareEnvironment, compareShiftMicros, parseApmScope, writeApmScope } from '../scope';

/**
 * What an APM page is looking at: the scope in its address, the time range
 * of the time picker, and the organization. A change of scope replaces the
 * address rather than adding to the history, as a filter does.
 */
export function useApmScope() {
  const [searchParams, setSearchParams] = useSearchParams();
  const scope = React.useMemo(() => parseApmScope(searchParams), [searchParams]);
  const window = useTimeStore((state) => state.window);
  const range = React.useMemo(() => resolveWindow(window), [window]);
  const orgId = useAuthStore((state) => state.ctx?.orgId ?? '');

  const setScope = React.useCallback(
    (patch: Partial<ApmScope>) =>
      setSearchParams((current) => writeApmScope(current, patch), {
        replace: true,
      }),
    [setSearchParams],
  );

  /** The stretch a comparison draws under the current one, the same one for another environment; `null` while there is none. */
  const compareRange = React.useMemo(() => {
    if (compareEnvironment(scope.compare)) return range;
    const shift = compareShiftMicros(scope.compare) / 1_000;
    if (!shift) return null;
    return {
      from: new Date(range.from.getTime() - shift),
      to: new Date(range.to.getTime() - shift),
    };
  }, [range, scope.compare]);

  return {
    scope,
    setScope,
    range,
    compareRange,
    orgId,
    searchParams,
    setSearchParams,
  };
}

/**
 * A drawer kept in the address, so a link opens it and Back closes it. A
 * drawer opened goes last in the address, as it goes on top of the drawers
 * already open: the order of their keys is the order they stack in.
 */
export function useAddressedDrawer(key: string) {
  const [searchParams, setSearchParams] = useSearchParams();
  const value = searchParams.get(key);

  const open = React.useCallback(
    (next: string, extra: Record<string, string> = {}) =>
      setSearchParams((current) => {
        const params = new URLSearchParams(current);
        params.delete(key);
        for (const name of Object.keys(extra)) params.delete(name);
        params.set(key, next);
        for (const [name, extraValue] of Object.entries(extra)) params.set(name, extraValue);
        return params;
      }),
    [key, setSearchParams],
  );

  const close = React.useCallback(
    (alsoClear: readonly string[] = []) =>
      setSearchParams((current) => {
        const params = new URLSearchParams(current);
        params.delete(key);
        for (const name of alsoClear) params.delete(name);
        return params;
      }),
    [key, setSearchParams],
  );

  return { value, open, close, searchParams };
}
