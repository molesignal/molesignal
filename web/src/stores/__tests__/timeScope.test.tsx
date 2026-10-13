import { act, cleanup, render, renderHook, screen } from '@testing-library/react';
import type * as React from 'react';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import {
  createTimeStore,
  DEFAULT_WINDOW,
  isDefaultTimeWindow,
  sameTimeWindow,
  TimeScopeContext,
  type TimeStoreApi,
  type TimeWindow,
  useTimeStore,
  useTimeStoreApi,
  writeTimeParams,
} from '@/stores/useTimeStore';

const SIX_HOURS: TimeWindow = { from: 'now-6h', to: 'now', mode: 'relative' };

function scoped(store: TimeStoreApi) {
  return function Wrapper({ children }: { children: React.ReactNode }) {
    return <TimeScopeContext.Provider value={store}>{children}</TimeScopeContext.Provider>;
  };
}

function Range() {
  const window = useTimeStore((state) => state.window);
  return <output>{`${window.from}..${window.to}`}</output>;
}

beforeEach(() => {
  useTimeStore.setState({ window: DEFAULT_WINDOW, anchor: null });
});

afterEach(() => {
  cleanup();
  useTimeStore.setState({ window: DEFAULT_WINDOW, anchor: null });
});

describe('time scope', () => {
  it('reads the app time range outside a scope', () => {
    render(<Range />);
    expect(screen.getByRole('status').textContent).toBe('now-1h..now');

    act(() => useTimeStore.getState().setWindow(SIX_HOURS));
    expect(screen.getByRole('status').textContent).toBe('now-6h..now');
  });

  it('reads and writes the scope own range inside one, and leaves the app range alone', () => {
    const tab = createTimeStore({ window: SIX_HOURS });
    const { result } = renderHook(
      () => ({
        window: useTimeStore((state) => state.window),
        setWindow: useTimeStore((state) => state.setWindow),
        api: useTimeStoreApi(),
      }),
      { wrapper: scoped(tab) },
    );

    expect(result.current.window).toEqual(SIX_HOURS);
    expect(result.current.api).toBe(tab);

    const day: TimeWindow = { from: 'now-24h', to: 'now', mode: 'relative' };
    act(() => result.current.setWindow(day));

    expect(result.current.window).toEqual(day);
    expect(tab.getState().window).toEqual(day);
    expect(useTimeStore.getState().window).toEqual(DEFAULT_WINDOW);
  });

  it('does not follow the app range from inside a scope', () => {
    const tab = createTimeStore();
    render(<Range />, { wrapper: scoped(tab) });

    act(() => useTimeStore.getState().setWindow(SIX_HOURS));

    expect(screen.getByRole('status').textContent).toBe('now-1h..now');
  });

  it('gives each store its own anchor', () => {
    const tab = createTimeStore();
    tab.getState().togglePin('2026-05-23T10:00:00Z');

    expect(tab.getState().anchor?.at).toBe('2026-05-23T10:00:00Z');
    expect(useTimeStore.getState().anchor).toBeNull();
  });
});

describe('time range in a URL', () => {
  it('compares ranges by value', () => {
    expect(sameTimeWindow(SIX_HOURS, { ...SIX_HOURS })).toBe(true);
    expect(sameTimeWindow(SIX_HOURS, DEFAULT_WINDOW)).toBe(false);
    expect(isDefaultTimeWindow({ from: 'now-1h', to: 'now', mode: 'relative' })).toBe(true);
  });

  it('writes a range and anchor the way the address carries them', () => {
    const params = new URLSearchParams('q=error&time=now-24h..now&anchor=old');

    writeTimeParams(params, SIX_HOURS, { at: '2026-05-23T10:00:00Z' });
    expect(params.toString()).toBe(
      'q=error&time=now-6h..now&anchor=2026-05-23T10%3A00%3A00Z',
    );

    // The default range and no anchor are not written at all.
    writeTimeParams(params, DEFAULT_WINDOW, null);
    expect(params.toString()).toBe('q=error');
  });
});
