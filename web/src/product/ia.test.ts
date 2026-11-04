import { describe, expect, it } from 'vitest';

import enNav from '@/i18n/en-us/nav.json';
import zhNav from '@/i18n/zh-cn/nav.json';

import { getProductNavItems, getProductRouteById, isProductRouteActive, PRODUCT_ROUTES } from './ia';

describe('product navigation', () => {
  it('keeps the primary observation workflow in the intended order', () => {
    expect(
      getProductNavItems('observe')
        .slice(0, 6)
        .map((item) => item.id),
    ).toEqual(['dashboards', 'metrics', 'logs', 'traces', 'apm', 'rum']);
  });

  it('keeps the reliability loop and AI operations in dedicated groups', () => {
    expect(getProductNavItems('reliability').map((item) => item.id)).toEqual([
      'alerts',
      'synthetics',
      'status.pages',
    ]);
    expect(getProductNavItems('ai_ops').map((item) => item.id)).toEqual([
      'agent',
    ]);
  });

  it('names every route in both locales', () => {
    const label = (nav: unknown, key: string) =>
      key.split('.').reduce<unknown>((node, part) => (node as Record<string, unknown> | undefined)?.[part], nav);

    expect(
      PRODUCT_ROUTES.filter((route) => !label(enNav, route.labelKey) || !label(zhNav, route.labelKey)).map(
        (route) => route.id,
      ),
    ).toEqual([]);
  });

  it('uses compact product labels in both locales', () => {
    expect([enNav.apm, enNav.rum]).toEqual(['APM', 'RUM']);
    expect([zhNav.apm, zhNav.rum]).toEqual(['应用性能', '用户体验']);
  });

  it('gives each admin area one sidebar entry, in the platform organization too', () => {
    expect(getProductNavItems('admin').map((item) => [item.id, item.path])).toEqual([
      ['iam', '/iam'],
      ['settings', '/settings'],
    ]);
  });

  it('keeps an entry lit on every page of the area it stands for', () => {
    const settings = getProductRouteById('settings')!;
    const iam = getProductRouteById('iam')!;
    const home = getProductRouteById('home')!;
    const logs = getProductRouteById('logs')!;

    // Settings opens General, but Audit and the notify pages belong to it too.
    expect(isProductRouteActive(settings, '/settings/general')).toBe(true);
    expect(isProductRouteActive(settings, '/settings/audit')).toBe(true);
    expect(isProductRouteActive(settings, '/settings/notify/connectors')).toBe(true);
    expect(isProductRouteActive(iam, '/iam/roles')).toBe(true);

    // A longer name that merely starts with the same letters is another area.
    expect(isProductRouteActive(settings, '/settings-archive')).toBe(false);
    expect(isProductRouteActive(settings, '/account/settings/profile')).toBe(false);
    expect(isProductRouteActive(iam, '/settings/general')).toBe(false);

    // Exact entries do not claim their children; ordinary entries do.
    expect(isProductRouteActive(home, '/home')).toBe(true);
    expect(isProductRouteActive(home, '/home/extra')).toBe(false);
    expect(isProductRouteActive(logs, '/logs')).toBe(true);
    expect(isProductRouteActive(logs, '/logs/')).toBe(true);
    expect(isProductRouteActive(logs, '/metrics')).toBe(false);
  });
});
