import '@/i18n';

import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, render, screen, within } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import * as iamApi from '@/api/iam';
import * as rolesApi from '@/api/roles';
import i18n from '@/i18n';
import type { PermissionCatalog, PermissionKey } from '@/product/permissions';
import { TooltipProvider } from '@/shell/ui/tooltip';

import { Roles } from './Roles';

vi.mock('@/api/roles', () => ({ list: vi.fn(), create: vi.fn(), update: vi.fn(), remove: vi.fn() }));
vi.mock('@/api/iam', () => ({ permissionCatalog: vi.fn() }));
vi.mock('@/product/actionAccess', () => ({
  useActionAccess: () => ({ allowed: true, disabled: false, reason: undefined }),
}));

const CATALOG: PermissionCatalog = {
  version: 10,
  permissions: [
    { key: 'sys.organizations.manage', scope: 'platform', domain: 'platform' },
    { key: 'streams.read', scope: 'organization', domain: 'observability' },
    { key: 'streams.query', scope: 'organization', domain: 'observability' },
    { key: 'alerts.read', scope: 'organization', domain: 'alerts' },
  ].map((permission) => ({
    ...permission,
    label_key: `permissions.${permission.key.replaceAll('.', '_')}`,
    description_key: `permissions_hint.${permission.key.replaceAll('.', '_')}`,
    builtin_roles: ['owner'],
  })) as PermissionCatalog['permissions'],
  bundles: [],
};

function role(key: string, permissions: string[]): rolesApi.IamRole {
  return {
    id: `role-${key}`,
    key,
    name: `${key} role`,
    description: '',
    builtin: true,
    role_type: 'organization',
    scope: 'organization',
    permissions: permissions as PermissionKey[],
    usage: { memberships: 1, api_tokens: 0, service_accounts: 0, invitations: 0, bindings: 1, total: 2 },
    created_at_micros: 1,
    updated_at_micros: 1,
  };
}

// A row is found by the role's key, which stands under its name.
const row = (key: string) => screen.getByText(key, { exact: true }).closest('tr') as HTMLElement;

beforeEach(async () => {
  await i18n.changeLanguage('en-US');
  vi.mocked(iamApi.permissionCatalog).mockResolvedValue(CATALOG);
  vi.mocked(rolesApi.list).mockResolvedValue([
    role('owner', CATALOG.permissions.map((permission) => permission.key)),
    // Everything an organization has, and nothing of the platform: not the whole catalog.
    role('admin', ['streams.read', 'streams.query', 'alerts.read']),
    role('viewer', ['streams.read']),
  ]);

  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <TooltipProvider delayDuration={0}>
        <MemoryRouter initialEntries={['/iam/roles']}>
          <Roles />
        </MemoryRouter>
      </TooltipProvider>
    </QueryClientProvider>,
  );
  await screen.findByText('owner', { exact: true });
});
afterEach(cleanup);

describe('the roles list', () => {
  it('says that the owner holds every permission there is, platform scope included', () => {
    expect(within(row('owner')).getByText('All 4 permissions')).toBeVisible();
    expect(row('owner').querySelector('[data-role-permissions="all"]')).not.toBeNull();
  });

  it('names what a role holds when it is not all of it', () => {
    expect(within(row('admin')).queryByText(/^All /)).toBeNull();
    expect(within(row('admin')).getByText('View alerts')).toBeVisible();
    expect(within(row('viewer')).queryByText(/^All /)).toBeNull();
  });
});
