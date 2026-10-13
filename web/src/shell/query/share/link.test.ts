import { describe, expect, it } from 'vitest';

import { timeFromSearchParams } from '@/shell/UrlHydration';
import { decodeFilters } from '@/shell/UrlHydration';
import { DEFAULT_WINDOW, type TimeWindow } from '@/stores/useTimeStore';

import { fixedWindow, queryShareLink, queryShareSearch, type QueryShare } from './link';

const NOW = new Date('2026-10-12T09:00:00.000Z');
const LAST_15M: TimeWindow = { mode: 'relative', from: 'now-15m', to: 'now' };

const share = (overrides: Partial<QueryShare> = {}): QueryShare => ({
  pathname: '/logs',
  own: new URLSearchParams({ stream: 'application_logs', q: "level = 'ERROR'" }),
  window: LAST_15M,
  anchor: null,
  filters: [],
  ...overrides,
});

describe('fixedWindow', () => {
  it('turns a window that moves into the moments it covers now', () => {
    expect(fixedWindow(LAST_15M, NOW)).toEqual({
      mode: 'absolute',
      from: '2026-10-12T08:45:00.000Z',
      to: '2026-10-12T09:00:00.000Z',
    });
  });

  it('leaves moments as they are', () => {
    const window: TimeWindow = { mode: 'absolute', from: '2026-10-01T00:00:00.000Z', to: '2026-10-02T00:00:00.000Z' };

    expect(fixedWindow(window, NOW)).toBe(window);
  });
});

describe('queryShareLink', () => {
  it('carries the query and the moments on screen, so the link shows the same data whenever it is opened', () => {
    const link = queryShareLink(share(), { fixed: true, origin: 'https://signal.example.com', now: NOW });
    const url = new URL(link);

    expect(url.origin + url.pathname).toBe('https://signal.example.com/logs');
    expect(url.searchParams.get('stream')).toBe('application_logs');
    expect(url.searchParams.get('q')).toBe("level = 'ERROR'");
    expect(url.searchParams.get('time')).toBe('2026-10-12T08:45:00.000Z..2026-10-12T09:00:00.000Z');
    // The app reads it back as the window it was written from.
    expect(timeFromSearchParams(url.searchParams).window).toEqual({
      mode: 'absolute',
      from: '2026-10-12T08:45:00.000Z',
      to: '2026-10-12T09:00:00.000Z',
    });
  });

  it('keeps a window that moves when asked to: the last fifteen minutes of whoever opens it', () => {
    const search = new URLSearchParams(queryShareSearch(share(), { fixed: false, now: NOW }));

    expect(search.get('time')).toBe('now-15m..now');
  });

  it('says nothing of the time for the window every page opens on, unless it is fixed', () => {
    const moving = new URLSearchParams(queryShareSearch(share({ window: DEFAULT_WINDOW }), { fixed: false, now: NOW }));
    const fixed = new URLSearchParams(queryShareSearch(share({ window: DEFAULT_WINDOW }), { fixed: true, now: NOW }));

    expect(moving.has('time')).toBe(false);
    expect(fixed.get('time')).toBe('2026-10-12T08:00:00.000Z..2026-10-12T09:00:00.000Z');
  });

  it('carries the pinned filters the query runs under, and the anchor', () => {
    const search = new URLSearchParams(
      queryShareSearch(
        share({
          filters: [{ key: 'env', value: 'prod', operator: '=' }, { key: 'region', value: 'eu', operator: '!=' }],
          anchor: { at: '2026-10-12T08:50:00.000Z' },
        }),
        { fixed: true, now: NOW },
      ),
    );

    expect(decodeFilters(search.get('filters') ?? '')).toEqual([
      { key: 'env', value: 'prod', operator: '=' },
      { key: 'region', value: 'eu', operator: '!=' },
    ]);
    expect(search.get('anchor')).toBe('2026-10-12T08:50:00.000Z');
  });

  it('writes the time and the filters from the stores, whatever the page passed among its own params', () => {
    const own = new URLSearchParams({ promql: 'up', time: 'now-7d..now', filters: '[["stale","x"]]', stack: 'abc' });
    const search = new URLSearchParams(queryShareSearch(share({ pathname: '/metrics', own }), { fixed: false, now: NOW }));

    expect(search.get('promql')).toBe('up');
    expect(search.get('time')).toBe('now-15m..now');
    expect(search.has('filters')).toBe(false);
    expect(search.has('stack')).toBe(false);
    // The page's params are not changed under it.
    expect(own.get('time')).toBe('now-7d..now');
  });

  it('is the bare address for a page with nothing asked of it', () => {
    expect(
      queryShareLink(share({ own: new URLSearchParams(), window: DEFAULT_WINDOW }), {
        fixed: false,
        origin: 'https://signal.example.com',
        now: NOW,
      }),
    ).toBe('https://signal.example.com/logs');
  });
});
