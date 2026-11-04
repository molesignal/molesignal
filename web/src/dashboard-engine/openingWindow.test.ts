import { renderHook } from '@testing-library/react';
import { describe, expect, it } from 'vitest';

import { DEFAULT_WINDOW, useTimeStore } from '@/stores/useTimeStore';

import { isBuiltinModel, useOpeningWindow } from './openingWindow';

const DAY = { defaultFrom: 'now-24h', defaultTo: 'now', timezone: 'browser' } as const;

describe('a built-in dashboard model', () => {
  it('is told by what the product wrote in it', () => {
    expect(isBuiltinModel({ extensions: { builtin: { key: 'rum-browser', version: 1 } } })).toBe(true);
    expect(isBuiltinModel({ extensions: {} })).toBe(false);
    expect(isBuiltinModel({})).toBe(false);
    expect(isBuiltinModel(undefined)).toBe(false);
  });
});

describe('the range a dashboard opens on', () => {
  it('is the one the dashboard was made for, when nothing else has chosen', () => {
    useTimeStore.getState().setWindow(DEFAULT_WINDOW);

    renderHook(() => useOpeningWindow('rum-browser', DAY, new URLSearchParams()));

    expect(useTimeStore.getState().window).toEqual({ from: 'now-24h', to: 'now', mode: 'relative' });
  });

  it('is the one in view when a person has chosen another', () => {
    const chosen = { from: 'now-7d', to: 'now', mode: 'relative' } as const;

    useTimeStore.getState().setWindow(chosen);

    renderHook(() => useOpeningWindow('rum-browser', DAY, new URLSearchParams()));

    expect(useTimeStore.getState().window).toEqual(chosen);
  });

  it('is the one the address names', () => {
    useTimeStore.getState().setWindow(DEFAULT_WINDOW);

    renderHook(() => useOpeningWindow('rum-browser', DAY, new URLSearchParams('time=now-15m..now')));

    expect(useTimeStore.getState().window).toEqual(DEFAULT_WINDOW);
  });

  it('is taken once for a dashboard, so a later choice of the person stays', () => {
    useTimeStore.getState().setWindow(DEFAULT_WINDOW);

    const { rerender } = renderHook(() => useOpeningWindow('rum-browser', DAY, new URLSearchParams()));

    useTimeStore.getState().setWindow(DEFAULT_WINDOW);
    rerender();

    expect(useTimeStore.getState().window).toEqual(DEFAULT_WINDOW);
  });

  it('is left alone while the dashboard is not read yet', () => {
    useTimeStore.getState().setWindow(DEFAULT_WINDOW);

    renderHook(() => useOpeningWindow(undefined, undefined, new URLSearchParams()));

    expect(useTimeStore.getState().window).toEqual(DEFAULT_WINDOW);
  });
});
