import { describe, expect, it } from 'vitest';

import enNav from '@/i18n/en-us/nav.json';
import zhNav from '@/i18n/zh-cn/nav.json';

import { getProductNavItems, getProductRouteById, isProductRouteActive } from './ia';

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

  it('uses compact product labels in both locales', () => {
    expect([enNav.apm, enNav.rum]).toEqual(['APM', 'RUM']);
    expect([zhNav.apm, zhNav.rum]).toEqual(['应用性能', '用户体验']);
  });

  it('uses organization management as the system Settings landing page', () => {
    expect(
      getProductNavItems('admin').find(
        (item) => item.id === 'settings.organization.management',
      ),
    ).toMatchObject({
      path: '/settings/organization_management',
      labelKey: 'settings',
      nav: true,
    });
  });

  it('exposes the system IAM landing page through the same permission model', () => {
    expect(
      getProductNavItems('admin').find(
        (item) => item.id === 'iam.organizations',
      ),
    ).toMatchObject({
      path: '/iam/organizations',
      labelKey: 'iam',
      nav: true,
    });
  });

  it('keeps an entry lit on every page of the area it stands for', () => {
    const settings = getProductRouteById('settings.general')!;
    const iam = getProductRouteById('iam.users')!;
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
