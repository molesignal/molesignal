import { describe, expect, it } from 'vitest';

import { pickActiveTab } from './matchTab';

const tabs = [
  { key: 'overview', to: '/synthetics/overview' },
  { key: 'checks', to: '/synthetics/checks' },
  { key: 'results', to: '/synthetics/results' },
];

describe('pickActiveTab', () => {
  it('marks the tab whose path is the current one', () => {
    expect(pickActiveTab(tabs, '/synthetics/results')).toBe('results');
  });

  it('keeps a tab active on the pages below it', () => {
    expect(pickActiveTab(tabs, '/synthetics/checks/3Jtc/results')).toBe('checks');
  });

  it('does not take a path that merely starts with the same letters', () => {
    expect(pickActiveTab(tabs, '/synthetics/checks-archive')).toBeUndefined();
  });

  it('ignores a trailing slash and the query string of the tab', () => {
    expect(
      pickActiveTab([{ key: 'apm', to: { pathname: '/apm/services', search: '?env=prod' } }], '/apm/services/'),
    ).toBe('apm');
    expect(pickActiveTab([{ key: 'apm', to: '/apm/services?env=prod' }], '/apm/services')).toBe('apm');
  });

  it('prefers the most specific tab when two both match', () => {
    const nested = [
      { key: 'checks', to: '/synthetics/checks' },
      { key: 'new', to: '/synthetics/checks/new' },
    ];
    expect(pickActiveTab(nested, '/synthetics/checks/new')).toBe('new');
    expect(pickActiveTab(nested, '/synthetics/checks/abc')).toBe('checks');
  });

  it('matches the exact path only when asked to', () => {
    const detail = [{ key: 'overview', to: '/checks/abc', end: true }];
    expect(pickActiveTab(detail, '/checks/abc')).toBe('overview');
    expect(pickActiveTab(detail, '/checks/abc/results')).toBeUndefined();
  });

  it('lets a tab say for itself whether it is active', () => {
    const owning = [
      { key: 'sessions', to: '/rum/sessions' },
      { key: 'performance', to: '/rum/performance/overview', active: true },
    ];
    expect(pickActiveTab(owning, '/rum/sessions')).toBe('performance');
    expect(
      pickActiveTab(
        [
          { key: 'sessions', to: '/rum/sessions' },
          { key: 'performance', to: '/rum/performance/overview', active: false },
        ],
        '/rum/performance/overview',
      ),
    ).toBeUndefined();
  });

  it('has nothing to mark on a route none of the tabs lead to', () => {
    expect(pickActiveTab(tabs, '/home')).toBeUndefined();
  });
});
