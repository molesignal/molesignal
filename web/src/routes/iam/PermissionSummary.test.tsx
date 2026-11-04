import '@/i18n';

import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import i18n from '@/i18n';
import type { PermissionCatalog } from '@/product/permissions';

import { PermissionSummary } from './PermissionSummary';

const CATALOG: PermissionCatalog = {
  version: 1,
  permissions: [
    { key: 'sys.organizations.read', scope: 'platform', domain: 'platform' },
    { key: 'alerts.rules.read', scope: 'organization', domain: 'alerts' },
    { key: 'dashboards.read', scope: 'organization', domain: 'dashboards' },
    { key: 'streams.query', scope: 'organization', domain: 'observability' },
    { key: 'streams.read', scope: 'organization', domain: 'observability' },
  ].map((permission) => ({
    ...permission,
    labelKey: `permissions.${permission.key.replaceAll('.', '_')}`,
    descriptionKey: `permissions_hint.${permission.key.replaceAll('.', '_')}`,
    builtinRoles: ['owner'],
  })) as PermissionCatalog['permissions'],
};

const ORGANIZATION = ['alerts.rules.read', 'dashboards.read', 'streams.query', 'streams.read'];

beforeEach(async () => {
  await i18n.changeLanguage('en-US');
});

afterEach(cleanup);

describe('PermissionSummary', () => {
  it('names the first three permissions as pills and counts the rest', () => {
    const { container } = render(<PermissionSummary catalog={CATALOG} permissions={ORGANIZATION} />);

    const pills = [...container.querySelectorAll('[data-permission-summary="some"] > span')];
    expect(pills.map((pill) => pill.textContent)).toEqual([
      'View alert rules',
      'View dashboards',
      'Query stream data',
      '+1',
    ]);
  });

  it('says a role holds everything when it holds the whole catalog', () => {
    render(
      <PermissionSummary
        catalog={CATALOG}
        permissions={['sys.organizations.read', ...ORGANIZATION]}
      />,
    );

    expect(screen.getByText('All 5 permissions')).toBeVisible();
  });

  it('measures a platform role against the platform permissions', () => {
    render(<PermissionSummary catalog={CATALOG} permissions={['sys.organizations.read']} scope="platform" />);

    expect(screen.getByText('All platform permissions (1)')).toBeVisible();
  });

  it('measures a policy against the organization permissions, all it can allow', () => {
    render(<PermissionSummary catalog={CATALOG} permissions={ORGANIZATION} scope="organization" />);

    expect(screen.getByText('All 4 organization permissions')).toBeVisible();
  });
});
