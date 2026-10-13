import { cleanup, renderHook } from '@testing-library/react';
import type * as React from 'react';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import {
  createTimeStore,
  DEFAULT_WINDOW,
  TimeScopeContext,
  type TimeWindow,
  useTimeStore,
} from '@/stores/useTimeStore';

import { useTraceTimeWindow } from './timeWindow';

const DAY: TimeWindow = { from: 'now-24h', to: 'now', mode: 'relative' };
const SIX_HOURS: TimeWindow = { from: 'now-6h', to: 'now', mode: 'relative' };

beforeEach(() => {
  useTimeStore.setState({ window: DEFAULT_WINDOW });
});

afterEach(() => {
  cleanup();
  useTimeStore.setState({ window: DEFAULT_WINDOW });
});

describe('useTraceTimeWindow', () => {
  it('looks back a day when the page opens on the default range', () => {
    const { result } = renderHook(() => useTraceTimeWindow(new URLSearchParams()));

    // The query runs on the day-long range at once, and the store has taken it.
    expect(result.current.effectiveTimeWindow).toEqual(DAY);
    expect(useTimeStore.getState().window).toEqual(DAY);
  });

  it('keeps a range chosen elsewhere', () => {
    useTimeStore.setState({ window: SIX_HOURS });

    const { result } = renderHook(() => useTraceTimeWindow(new URLSearchParams()));

    expect(result.current.effectiveTimeWindow).toEqual(SIX_HOURS);
    expect(useTimeStore.getState().window).toEqual(SIX_HOURS);
  });

  it('keeps the default hour when the address asks for it by name', () => {
    const { result } = renderHook(() =>
      useTraceTimeWindow(new URLSearchParams('from=now-1h&to=now')),
    );

    expect(result.current.effectiveTimeWindow).toEqual(DEFAULT_WINDOW);
    expect(useTimeStore.getState().window).toEqual(DEFAULT_WINDOW);
  });

  it('sets the range of the query tab it is in, not the app one', () => {
    const tab = createTimeStore();
    const wrapper = ({ children }: { children: React.ReactNode }) => (
      <TimeScopeContext.Provider value={tab}>{children}</TimeScopeContext.Provider>
    );

    renderHook(() => useTraceTimeWindow(new URLSearchParams()), { wrapper });

    expect(tab.getState().window).toEqual(DAY);
    expect(useTimeStore.getState().window).toEqual(DEFAULT_WINDOW);
  });
});
