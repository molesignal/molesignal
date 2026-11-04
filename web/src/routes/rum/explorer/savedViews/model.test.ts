import { describe, expect, it } from 'vitest';

import {
  canSaveView,
  MAX_SAVED_VIEWS,
  MAX_VIEW_SEARCH,
  parseSavedViews,
  viewSearchOf,
  withoutSavedView,
  withSavedView,
} from './model';

describe('saved views of the explorer', () => {
  it('keeps what shows the view, and a range only when it moves', () => {
    const share = new URLSearchParams({ type: 'errors', q: "env = 'prod'" });

    expect(viewSearchOf(share, 'now-1h..now')).toBe('q=env+%3D+%27prod%27&time=now-1h..now&type=errors');
    expect(viewSearchOf(share, '2026-10-01T00:00:00Z..2026-10-02T00:00:00Z')).toBe('q=env+%3D+%27prod%27&type=errors');
    expect(viewSearchOf(share, null)).toBe('q=env+%3D+%27prod%27&type=errors');
  });

  it('adds a view, and a name that is kept takes the new address', () => {
    const one = withSavedView([], ' Slow pages ', 'type=views');

    expect(one).toEqual([{ id: 'view-1', name: 'Slow pages', search: 'type=views' }]);

    const again = withSavedView(one, 'slow pages', 'type=resources');

    expect(again).toEqual([{ id: 'view-1', name: 'slow pages', search: 'type=resources' }]);

    const two = withSavedView(again, 'Errors', 'type=errors');

    expect(two.map((view) => view.id)).toEqual(['view-1', 'view-2']);
    expect(withSavedView(withoutSavedView(two, 'view-1'), 'More', '')).toHaveLength(2);
    expect(new Set(withSavedView(withoutSavedView(two, 'view-1'), 'More', '').map((view) => view.id)).size).toBe(2);
  });

  it('only keeps what the server takes', () => {
    const full = Array.from({ length: MAX_SAVED_VIEWS }, (_, index) => ({ id: `${index}`, name: `v${index}`, search: '' }));

    expect(canSaveView([], 'x', 'type=views')).toBe(true);
    expect(canSaveView([], '  ', 'type=views')).toBe(false);
    expect(canSaveView([], 'x', 'a'.repeat(MAX_VIEW_SEARCH + 1))).toBe(false);
    expect(canSaveView(full, 'x', '')).toBe(false);
    expect(canSaveView(full, 'v1', '')).toBe(true);
  });

  it('reads what is kept, and nothing else', () => {
    expect(parseSavedViews({ views: [{ id: 'a', name: 'A', search: '' }, { id: 1 }] })).toEqual({ views: [{ id: 'a', name: 'A', search: '' }] });
    expect(parseSavedViews({ other: [] })).toBeNull();
    expect(parseSavedViews(null)).toBeNull();
  });
});
