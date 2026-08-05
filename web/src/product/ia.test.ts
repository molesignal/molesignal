import { describe, expect, it } from 'vitest';

import enNav from '@/i18n/en-us/nav.json';
import zhNav from '@/i18n/zh-cn/nav.json';

import { getProductNavItems } from './ia';

describe('product navigation', () => {
  it('keeps the primary analysis workflow in the intended order', () => {
    expect(
      getProductNavItems('investigate')
        .slice(0, 6)
        .map((item) => item.id),
    ).toEqual(['dashboards', 'metrics', 'logs', 'traces', 'apm', 'rum']);
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
});
