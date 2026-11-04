import { cleanup, renderHook } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

// The shapes of the world are what is being loaded: the module is told by its being evaluated,
// which happens once for each time the modules are reset and the module is mocked again.
const loaded = vi.fn();

type IdleWindow = { requestIdleCallback?: unknown; cancelIdleCallback?: unknown };

const preload = () => import('./preload');

beforeEach(() => {
  vi.resetModules();
  vi.doMock('./world', () => {
    loaded();

    return {};
  });
  vi.useFakeTimers();
  loaded.mockClear();
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  delete (window as IdleWindow).requestIdleCallback;
  delete (window as IdleWindow).cancelIdleCallback;
});

describe('loading the shapes of the world ahead of a map', () => {
  it('starts at once when it is asked to, and only once', async () => {
    const { preloadWorldMap } = await preload();

    preloadWorldMap();
    preloadWorldMap();

    await vi.waitFor(() => expect(loaded).toHaveBeenCalledTimes(1));
  });

  it('waits for a timer where the browser has no idle time', async () => {
    const { usePreloadWorldMap } = await preload();

    renderHook(() => usePreloadWorldMap());

    expect(loaded).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(250);
    await vi.waitFor(() => expect(loaded).toHaveBeenCalledTimes(1));
  });

  it('is left to the browser where it has an idle time, and takes back what the page left before it came', async () => {
    const request = vi.fn(() => 7);
    const cancel = vi.fn();
    Object.assign(window, { requestIdleCallback: request, cancelIdleCallback: cancel });
    const { usePreloadWorldMap } = await preload();

    const { unmount } = renderHook(() => usePreloadWorldMap());

    expect(request).toHaveBeenCalledTimes(1);
    unmount();
    expect(cancel).toHaveBeenCalledWith(7);
    expect(loaded).not.toHaveBeenCalled();
  });

  it('does nothing when the page has no map to draw', async () => {
    const { usePreloadWorldMap } = await preload();

    renderHook(() => usePreloadWorldMap(false));

    await vi.advanceTimersByTimeAsync(5_000);
    expect(loaded).not.toHaveBeenCalled();
  });

  it('is cancelled by leaving the page before its time', async () => {
    const { usePreloadWorldMap } = await preload();

    const { unmount } = renderHook(() => usePreloadWorldMap());

    unmount();
    await vi.advanceTimersByTimeAsync(5_000);
    expect(loaded).not.toHaveBeenCalled();
  });
});
