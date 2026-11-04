import { describe, expect, it } from 'vitest';

import { suggestRoutes } from './suggestRoutes';

const routes = [
  { path: '/alerts/rules' },
  { path: '/alerts/silences' },
  { path: '/settings/notify/connectors' },
  { path: '/settings/general' },
  { path: '/pipelines/connectors' },
  { path: '/home' },
];
const paths = (found: typeof routes) => found.map((route) => route.path);

describe('suggestRoutes', () => {
  it('finds the page that shares the words of the address', () => {
    expect(paths(suggestRoutes('/alerts/notify/connectors', routes))[0]).toBe('/settings/notify/connectors');
  });

  it('allows a typo', () => {
    expect(paths(suggestRoutes('/alerts/silence', routes))[0]).toBe('/alerts/silences');
    expect(paths(suggestRoutes('/setings/genral', routes))).toEqual(['/settings/general']);
  });

  it('says nothing when nothing is close', () => {
    expect(suggestRoutes('/this-route-does-not-exist', routes)).toEqual([]);
    expect(suggestRoutes('/', routes)).toEqual([]);
  });

  it('keeps to the limit', () => {
    expect(suggestRoutes('/alerts', routes, 1)).toHaveLength(1);
  });
});
