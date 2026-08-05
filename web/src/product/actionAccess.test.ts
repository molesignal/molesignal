import { describe, expect, it } from 'vitest';

import {
  resolveActionAccess,
  restrictActionAccess,
} from './actionAccess';
import type { ProductAccess } from './access';
import type { PermissionKey } from './permissions';

const copy = {
  loading: 'Permissions are loading',
  permissionRequired: (permission: string) => `Requires ${permission}`,
  featureRequired: (feature: string) => `Requires feature ${feature}`,
};

function access(
  permissions: PermissionKey[],
  features: string[] = [],
): ProductAccess {
  return {
    organizationId: 'org-a',
    role: 'Viewer',
    scope: 'organization',
    permissions: new Set(permissions),
    features: new Set(features),
    version: 1,
    routeCatalogVersion: 1,
    routes: [],
    status: 'ready',
  };
}

describe('action access decisions', () => {
  it.each([
    {
      role: 'Viewer',
      snapshot: access(['dashboards.read']),
      permission: 'dashboards.create',
      allowed: false,
    },
    {
      role: 'Editor',
      snapshot: access(['dashboards.read', 'dashboards.create']),
      permission: 'dashboards.create',
      allowed: true,
    },
    {
      role: 'Admin',
      snapshot: access(['org.settings.read', 'org.settings.manage']),
      permission: 'org.settings.manage',
      allowed: true,
    },
    {
      role: 'Platform Owner',
      snapshot: {
        ...access(['sys.settings.manage']),
        organizationId: '_sys',
        scope: 'system' as const,
      },
      permission: 'sys.settings.manage',
      allowed: true,
    },
  ] as const)(
    'resolves $role actions from permission keys',
    ({ snapshot, permission, allowed }) => {
      const result = resolveActionAccess(
        snapshot,
        { permission },
        copy,
      );
      expect(result.allowed).toBe(allowed);
      expect(result.disabled).toBe(!allowed);
      if (!allowed) expect(result.reason).toContain(permission);
    },
  );

  it('reports license and workflow restrictions without hiding the action', () => {
    const missingFeature = resolveActionAccess(
      access(['org.settings.manage']),
      {
        permission: 'org.settings.manage',
        feature: 'domain_management',
      },
      copy,
    );
    expect(missingFeature).toEqual({
      allowed: false,
      disabled: true,
      reason: 'Requires feature domain_management',
    });

    const wrongState = restrictActionAccess(
      resolveActionAccess(
        access(['pipelines.run']),
        { permission: 'pipelines.run' },
        copy,
      ),
      false,
      'Pipeline is already running',
    );
    expect(wrongState).toEqual({
      allowed: false,
      disabled: true,
      reason: 'Pipeline is already running',
    });
  });
});
