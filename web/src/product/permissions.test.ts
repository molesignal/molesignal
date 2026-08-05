import { describe, expect, it } from 'vitest';

import {
  isPermissionKey,
  normalizePermissionKeys,
  permissionDefinitionsByDomain,
  type PermissionCatalog,
} from './permissions';

const catalog: PermissionCatalog = {
  version: 4,
  permissions: [
    {
      key: 'sys.licenses.manage',
      scope: 'platform',
      domain: 'platform',
      label_key: 'permissions.sys_licenses_manage',
      description_key: 'permissions_hint.sys_licenses_manage',
      builtin_roles: ['platform_owner'],
    },
    {
      key: 'streams.query',
      scope: 'organization',
      domain: 'observability',
      label_key: 'permissions.streams_query',
      description_key: 'permissions_hint.streams_query',
      builtin_roles: ['owner', 'admin', 'editor', 'viewer'],
    },
    {
      key: 'streams.write',
      scope: 'organization',
      domain: 'observability',
      label_key: 'permissions.streams_write',
      description_key: 'permissions_hint.streams_write',
      builtin_roles: ['owner', 'admin', 'editor'],
    },
    {
      key: 'iam.roles.read',
      scope: 'organization',
      domain: 'iam',
      label_key: 'permissions.iam_roles_read',
      description_key: 'permissions_hint.iam_roles_read',
      builtin_roles: ['owner', 'admin'],
    },
  ],
  bundles: [
    {
      key: 'observer',
      label_key: 'roles.bundles.observer',
      description_key: 'roles.bundles_hint.observer',
      permissions: ['streams.query'],
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
