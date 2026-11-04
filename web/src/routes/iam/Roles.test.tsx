import '@/i18n';

import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, render, screen, within } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import * as iamApi from '@/api/iam';
import * as policiesApi from '@/api/policies';
import * as rolesApi from '@/api/roles';
import i18n from '@/i18n';
import type { PermissionCatalog, PermissionKey } from '@/product/permissions';
import { TooltipProvider } from '@/shell/ui/tooltip';

import { Roles } from './Roles';

vi.mock('@/api/roles', () => ({ list: vi.fn(), create: vi.fn(), update: vi.fn(), remove: vi.fn() }));
vi.mock('@/api/iam', () => ({ permissionCatalog: vi.fn() }));
vi.mock('@/api/policies', () => ({
  POLICIES_QUERY_KEY: ['iam', 'policies'],
  list: vi.fn(),
  create: vi.fn(),
  update: vi.fn(),
  remove: vi.fn(),
}));
vi.mock('@/product/actionAccess', () => ({
  useActionAccess: () => ({ allowed: true, disabled: false, reason: undefined }),
}));

const CATALOG: PermissionCatalog = {
  version: 10,
  permissions: [
    { key: 'sys.organizations.read', scope: 'platform', domain: 'platform' },
    { key: 'org.delete', scope: 'organization', domain: 'organization' },
    { key: 'streams.read', scope: 'organization', domain: 'observability' },
    { key: 'streams.query', scope: 'organization', domain: 'observability' },
    { key: 'alerts.rules.read', scope: 'organization', domain: 'alerts' },
  ].map((permission) => ({
    ...permission,
    labelKey: `permissions.${permission.key.replaceAll('.', '_')}`,
    descriptionKey: `permissions_hint.${permission.key.replaceAll('.', '_')}`,
    builtinRoles: ['owner'],
  })) as PermissionCatalog['permissions'],
};

const OBSERVER: policiesApi.IamPolicy = {
  id: 'builtin:readonly_observer',
  key: 'readonly_observer',
  name: 'Read-only observer',
  description: '',
  builtin: true,
  document: { Version: '1', Statement: [{ Effect: 'Allow', Action: ['alerts.rules.read'] }] },
  permissions: ['alerts.rules.read'],
  usage: { roles: 1, tokens: 0, total: 1 },
  createdAt: 1,
  updatedAt: 1,
};

function role(
  key: string,
  permissions: string[],
  policies: policiesApi.IamPolicy[] = [],
): rolesApi.IamRole {
  return {
    id: `role-${key}`,
    key,
    name: `${key} role`,
    description: '',
    builtin: true,
    roleType: 'organization',
    scope: 'organization',
    permissions: permissions as PermissionKey[],
    policyIds: policies.map((policy) => policy.id),
    effectivePermissions: [
      ...new Set([...permissions, ...policies.flatMap((policy) => policy.permissions)]),
    ].sort() as PermissionKey[],
    usage: { memberships: 1, serviceAccounts: 0, invitations: 0, bindings: 1, total: 2 },
    createdAt: 1,
    updatedAt: 1,
  };
}

// A row is found by the role's key, which stands under its name.
const row = (key: string) => screen.getByText(key, { exact: true }).closest('tr') as HTMLElement;

beforeEach(async () => {
  await i18n.changeLanguage('en-US');
  vi.mocked(iamApi.permissionCatalog).mockResolvedValue(CATALOG);
  vi.mocked(policiesApi.list).mockResolvedValue([OBSERVER]);
  vi.mocked(rolesApi.list).mockResolvedValue([
    role('owner', ['org.delete', 'streams.read', 'streams.query', 'alerts.rules.read']),
    role('admin', ['streams.read', 'streams.query', 'alerts.rules.read']),
    role('viewer', ['streams.read']),
    role('readers', ['streams.read'], [OBSERVER]),
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
  it('says that the owner holds every permission of the organization', () => {
    expect(within(row('owner')).getByText('All 4 organization permissions')).toBeVisible();
    expect(row('owner').querySelector('[data-permission-summary="all"]')).not.toBeNull();
  });

  it('names what a role holds when it is not all of it', () => {
    expect(within(row('admin')).queryByText(/^All /)).toBeNull();
    expect(within(row('admin')).getByText('View alert rules')).toBeVisible();
    expect(within(row('viewer')).queryByText(/^All /)).toBeNull();
  });

  it('names the policies a role attaches and counts what they grant', () => {
    expect(within(row('readers')).getByText('Read-only observer')).toBeVisible();
    expect(within(row('readers')).getByText('View alert rules')).toBeVisible();
    expect(within(row('viewer')).queryByText('Read-only observer')).toBeNull();
  });
});
