import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import {
  LAST_VISITED_HOME,
  rememberLastVisitedRoute,
  resolveDefaultHomeRoute,
} from './homeRoute';

const STORAGE_KEY = 'molesignal-last-routes';

const stored = () => JSON.parse(localStorage.getItem(STORAGE_KEY) ?? '{}') as Record<string, string>;

beforeEach(() => {
  localStorage.clear();
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe('last visited route', () => {
  it('keeps every user and org in one storage key', () => {
    rememberLastVisitedRoute('u1', 'o1', '/logs?stream=app');
    rememberLastVisitedRoute('u1', 'o2', '/alerts');
    rememberLastVisitedRoute('u2', 'o1', '/dashboards');

    expect(localStorage.length).toBe(1);
    expect(stored()).toEqual({
      'u1:o1': '/logs?stream=app',
      'u1:o2': '/alerts',
      'u2:o1': '/dashboards',
    });
  });

  it('resolves the route of the matching user and org', () => {
    rememberLastVisitedRoute('u1', 'o1', '/logs');
    rememberLastVisitedRoute('u1', 'o2', '/alerts');

    expect(resolveDefaultHomeRoute(LAST_VISITED_HOME, 'u1', 'o2')).toBe('/alerts');
    expect(resolveDefaultHomeRoute(LAST_VISITED_HOME, 'u1', 'o1')).toBe('/logs');
  });

  it('falls back to home when there is nothing to resume', () => {
    expect(resolveDefaultHomeRoute(LAST_VISITED_HOME, 'u1', 'o1')).toBe('/home');
  });

  it('uses a fixed preference as it is, and ignores one that is not a path', () => {
    expect(resolveDefaultHomeRoute('/metrics', 'u1', 'o1')).toBe('/metrics');
    expect(resolveDefaultHomeRoute('metrics', 'u1', 'o1')).toBe('/home');
  });

  it('does not remember sign-in, account settings or the bare root', () => {
    for (const route of ['/', '/signin?next=%2Fhome', '/signup', '/account/settings/profile']) {
      rememberLastVisitedRoute('u1', 'o1', route);
    }
    rememberLastVisitedRoute('', 'o1', '/logs');
    rememberLastVisitedRoute('u1', '', '/logs');
    rememberLastVisitedRoute('u1', 'o1', 'logs');

    expect(localStorage.getItem(STORAGE_KEY)).toBeNull();
  });

  it('does not write again when the route has not changed', () => {
    rememberLastVisitedRoute('u1', 'o1', '/logs');
    const setItem = vi.spyOn(Object.getPrototypeOf(localStorage) as Storage, 'setItem');

    rememberLastVisitedRoute('u1', 'o1', '/logs');
    expect(setItem).not.toHaveBeenCalled();

    rememberLastVisitedRoute('u1', 'o1', '/metrics');
    expect(setItem).toHaveBeenCalledTimes(1);
  });

  it('recovers from unreadable or malformed storage', () => {
    localStorage.setItem(STORAGE_KEY, '{not json');
    expect(resolveDefaultHomeRoute(LAST_VISITED_HOME, 'u1', 'o1')).toBe('/home');

    localStorage.setItem(STORAGE_KEY, JSON.stringify(['/logs']));
    expect(resolveDefaultHomeRoute(LAST_VISITED_HOME, 'u1', 'o1')).toBe('/home');

    localStorage.setItem(STORAGE_KEY, JSON.stringify({ 'u1:o1': 42, 'u1:o2': 'javascript:1', 'u1:o3': '/ok' }));
    expect(resolveDefaultHomeRoute(LAST_VISITED_HOME, 'u1', 'o1')).toBe('/home');
    expect(resolveDefaultHomeRoute(LAST_VISITED_HOME, 'u1', 'o2')).toBe('/home');
    expect(resolveDefaultHomeRoute(LAST_VISITED_HOME, 'u1', 'o3')).toBe('/ok');

    rememberLastVisitedRoute('u1', 'o1', '/logs');
    expect(stored()).toEqual({ 'u1:o3': '/ok', 'u1:o1': '/logs' });
  });

  describe('keys written by earlier builds', () => {
    beforeEach(() => {
      localStorage.setItem('molesignal-last-route:u1:o1', '/logs');
      localStorage.setItem('molesignal-last-route:u1:o2', '/alerts');
      localStorage.setItem('molesignal-theme', 'light');
    });

    it('are folded into the shared key and removed when read', () => {
      expect(resolveDefaultHomeRoute(LAST_VISITED_HOME, 'u1', 'o2')).toBe('/alerts');

      expect(stored()).toEqual({ 'u1:o1': '/logs', 'u1:o2': '/alerts' });
      expect(localStorage.getItem('molesignal-last-route:u1:o1')).toBeNull();
      expect(localStorage.getItem('molesignal-last-route:u1:o2')).toBeNull();
      // Unrelated keys stay where they are.
      expect(localStorage.getItem('molesignal-theme')).toBe('light');
    });

    it('are folded in when a route is remembered first', () => {
      rememberLastVisitedRoute('u2', 'o1', '/dashboards');

      expect(stored()).toEqual({
        'u1:o1': '/logs',
        'u1:o2': '/alerts',
        'u2:o1': '/dashboards',
      });
      expect(localStorage.length).toBe(2);
    });

    it('never override a route already in the shared key', () => {
      localStorage.setItem(STORAGE_KEY, JSON.stringify({ 'u1:o1': '/newer' }));

      expect(resolveDefaultHomeRoute(LAST_VISITED_HOME, 'u1', 'o1')).toBe('/newer');
      expect(resolveDefaultHomeRoute(LAST_VISITED_HOME, 'u1', 'o2')).toBe('/alerts');
    });

    it('are dropped when they hold something that is not a route', () => {
      localStorage.setItem('molesignal-last-route:u3:o1', 'not-a-route');

      expect(resolveDefaultHomeRoute(LAST_VISITED_HOME, 'u3', 'o1')).toBe('/home');
      expect(localStorage.getItem('molesignal-last-route:u3:o1')).toBeNull();
      expect(stored()['u3:o1']).toBeUndefined();
    });
  });
});
