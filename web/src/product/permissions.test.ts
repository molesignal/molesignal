import { describe, expect, it } from 'vitest';

import {
  isOrganizationNarrowable,
  isPermissionKey,
  normalizePermissionKeys,
  permissionDefinitionsByDomain,
  type PermissionCatalog,
} from './permissions';

const catalog: PermissionCatalog = {
  version: 4,
  permissions: [
    {
      key: 'sys.telemetry.manage',
      scope: 'platform',
      domain: 'platform',
      labelKey: 'permissions.sys_telemetry_manage',
      descriptionKey: 'permissions_hint.sys_telemetry_manage',
      builtinRoles: ['platform_owner'],
    },
    {
      key: 'streams.query',
      scope: 'organization',
      domain: 'observability',
      labelKey: 'permissions.streams_query',
      descriptionKey: 'permissions_hint.streams_query',
      builtinRoles: ['owner', 'admin', 'editor', 'viewer'],
    },
    {
      key: 'logs.write',
      scope: 'organization',
      domain: 'observability',
      labelKey: 'permissions.logs_write',
      descriptionKey: 'permissions_hint.logs_write',
      builtinRoles: ['owner', 'admin', 'editor'],
    },
    {
      key: 'iam.roles.read',
      scope: 'organization',
      domain: 'iam',
      labelKey: 'permissions.iam_roles_read',
      descriptionKey: 'permissions_hint.iam_roles_read',
      builtinRoles: ['owner', 'admin'],
    },
  ],
};

describe('IAM permission catalog helpers', () => {
  it('validates canonical keys against the database response', () => {
    expect(catalog.version).toBeGreaterThan(0);
    expect(
      catalog.permissions.every((permission) =>
        isPermissionKey(permission.key, catalog),
      ),
    ).toBe(true);
    expect(isPermissionKey('not.real', catalog)).toBe(false);
  });

  it('lists the permissions of the scope a session counts', () => {
    const groups = permissionDefinitionsByDomain(catalog, 'platform');

    expect(groups.map((group) => group.domain)).toEqual(['platform']);
    expect(groups[0]?.permissions.map((permission) => permission.key)).toEqual([
      'sys.telemetry.manage',
    ]);
  });

  it('knows which platform permissions a grant can narrow to one organization', () => {
    expect(isOrganizationNarrowable('sys.organizations.update')).toBe(true);
    expect(isOrganizationNarrowable('sys.organizations.create')).toBe(false);
    expect(isOrganizationNarrowable('sys.settings.retention.manage')).toBe(false);
  });

  it('groups organization permissions and normalizes registered keys', () => {
    expect(permissionDefinitionsByDomain(catalog)).toHaveLength(2);
    expect(
      normalizePermissionKeys(
        [' streams.query ', 'streams.query', 'not.real'],
        catalog,
      ),
    ).toEqual(['streams.query']);
  });
});
