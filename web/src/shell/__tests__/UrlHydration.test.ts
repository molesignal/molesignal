import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import { hydrateFromSearchParams, timeFromSearchParams } from '@/shell/UrlHydration';
import { useFiltersStore } from '@/stores/useFiltersStore';
import { DEFAULT_WINDOW, useTimeStore } from '@/stores/useTimeStore';

beforeEach(() => {
  useTimeStore.setState({ window: DEFAULT_WINDOW, anchor: null });
  useFiltersStore.getState().clearFilters();
});

afterEach(() => {
  useTimeStore.setState({ window: DEFAULT_WINDOW, anchor: null });
  useFiltersStore.getState().clearFilters();
});

describe('timeFromSearchParams', () => {
  it('reads the canonical `time` form', () => {
    expect(timeFromSearchParams(new URLSearchParams('time=now-6h..now'))).toEqual({
      window: { from: 'now-6h', to: 'now', mode: 'relative' },
    });
    expect(
      timeFromSearchParams(
        new URLSearchParams('time=2026-05-23T09:00:00Z..2026-05-23T10:00:00Z'),
      ).window?.mode,
    ).toBe('absolute');
  });

  it('reads explicit `from` / `to` when there is no `time`', () => {
    expect(timeFromSearchParams(new URLSearchParams('from=now-15m&to=now')).window).toEqual({
      from: 'now-15m',
      to: 'now',
      mode: 'relative',
    });
    // `time` wins when both are present.
    expect(
      timeFromSearchParams(new URLSearchParams('time=now-6h..now&from=now-15m&to=now')).window?.from,
    ).toBe('now-6h');
  });

  it('says nothing about time when the address does not', () => {
    expect(timeFromSearchParams(new URLSearchParams('q=error'))).toEqual({});
    expect(timeFromSearchParams(new URLSearchParams('time=broken'))).toEqual({});
  });

  it('tells an anchor that is set from one that is cleared', () => {
    expect(timeFromSearchParams(new URLSearchParams('anchor=2026-05-23T10:00:00Z')).anchor).toEqual({
      at: '2026-05-23T10:00:00Z',
    });
    expect(timeFromSearchParams(new URLSearchParams('anchor=')).anchor).toBeNull();
    expect('anchor' in timeFromSearchParams(new URLSearchParams('q=error'))).toBe(false);
  });
});

describe('hydrateFromSearchParams', () => {
  it('writes the time range of the address into the store', () => {
    hydrateFromSearchParams(new URLSearchParams('time=now-6h..now&anchor=2026-05-23T10:00:00Z'));

    expect(useTimeStore.getState().window).toEqual({ from: 'now-6h', to: 'now', mode: 'relative' });
    expect(useTimeStore.getState().anchor?.at).toBe('2026-05-23T10:00:00Z');
  });

  it('leaves a value the store already holds alone, so nothing that reads it runs again', () => {
    hydrateFromSearchParams(
      new URLSearchParams('time=now-6h..now&anchor=2026-05-23T10:00:00Z&filters=%5B%5B%22service%22%2C%22%3D%22%2C%22checkout%22%5D%5D'),
    );
    const before = useTimeStore.getState();
    const filters = useFiltersStore.getState().filters;
    expect(filters).toEqual([{ key: 'service', operator: '=', value: 'checkout' }]);

    hydrateFromSearchParams(
      new URLSearchParams('time=now-6h..now&anchor=2026-05-23T10:00:00Z&filters=%5B%5B%22service%22%2C%22%3D%22%2C%22checkout%22%5D%5D&q=other'),
    );

    // Same objects: a reader keyed on identity is not told anything changed.
    expect(useTimeStore.getState().window).toBe(before.window);
    expect(useTimeStore.getState().anchor).toBe(before.anchor);
    expect(useFiltersStore.getState().filters).toBe(filters);
  });

  it('clears the anchor and filters when the address clears them', () => {
    hydrateFromSearchParams(
      new URLSearchParams('anchor=2026-05-23T10:00:00Z&filters=%5B%5B%22a%22%2C%22%3D%22%2C%22b%22%5D%5D'),
    );

    hydrateFromSearchParams(new URLSearchParams('anchor=&filters='));

    expect(useTimeStore.getState().anchor).toBeNull();
    expect(useFiltersStore.getState().filters).toEqual([]);
  });
});
