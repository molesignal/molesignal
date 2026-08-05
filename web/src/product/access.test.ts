import { describe, expect, it } from 'vitest';

import type { IamCapabilitySnapshot, IamRouteAccess } from '@/api/iam';

import {
  accessibleProductNavigation,
  accessFromSnapshot,
  canAccessProductPath,
  deniedProductRouteFallback,
  isKnownProductPath,
  routePatternMatches,
  type ProductAccess,
} from './access';

function decision(
  id: string,
  path_pattern: string,
  allowed: boolean,
  navigation_group?: IamRouteAccess['navigation_group'],
  navigation_position?: number,
): IamRouteAccess {
  return {
    id,
    path_pattern,
    allowed,
    ...(navigation_group ? { navigation_group } : {}),
    ...(navigation_position === undefined ? {} : { navigation_position }),
  };
}

function access(routes: IamRouteAccess[]): ProductAccess {
  return {
    organizationId: 'org-a',
    role: 'Viewer',
    scope: 'organization',
    permissions: new Set(['streams.query']),
    features: new Set(),
    version: 3,
    routeCatalogVersion: 7,
    routes,
    status: 'ready',
  };
}

describe('database-backed product route access', () => {
  it('uses server decisions rather than re-evaluating permissions in React', () => {
    const snapshot = access([
      decision('dashboards', '/dashboards', true),
      decision('dashboard.new.edit', '/dashboards/new/edit', false),
    ]);
    expect(canAccessProductPath('/dashboards', snapshot)).toBe(true);
    expect(canAccessProductPath('/dashboards/new/edit', snapshot)).toBe(false);

    const permissionsChanged = {
      ...snapshot,
      permissions: new Set(['dashboards.create'] as const),
    };
    expect(
      canAccessProductPath('/dashboards/new/edit', permissionsChanged),
    ).toBe(false);
  });

  it('selects the most specific matching route policy', () => {
    const snapshot = access([
      decision('settings.section', '/settings/:section', true),
      decision('settings.license', '/settings/license', false),
      decision('notify', '/settings/notify/*', true),
    ]);
    expect(canAccessProductPath('/settings/general', snapshot)).toBe(true);
    expect(canAccessProductPath('/settings/license', snapshot)).toBe(false);
    expect(
      canAccessProductPath('/settings/notify/connectors', snapshot),
    ).toBe(true);
  });

  it('matches literals, parameters, and trailing wildcards', () => {
    expect(routePatternMatches('/traces/:id', '/traces/abc')).toBe(true);
    expect(routePatternMatches('/traces/:id', '/traces/abc/events')).toBe(
      false,
    );
    expect(
      routePatternMatches('/settings/notify/*', '/settings/notify/connectors'),
    ).toBe(true);
    expect(routePatternMatches('/', '/')).toBe(true);
  });

  it('renders navigation groups and ordering from the server catalog', () => {
    const snapshot = access([
      decision('metrics', '/metrics', true, 'investigate', 20),
      decision('dashboards', '/dashboards', true, 'investigate', 10),
      decision('logs', '/logs', false, 'investigate', 30),
    ]);
    expect(
      accessibleProductNavigation(snapshot, 'investigate').map(
        (route) => route.id,
      ),
    ).toEqual(['dashboards', 'metrics']);
  });

  it('fails closed when a registered route is missing from the DB catalog', () => {
    const snapshot = access([]);
    expect(isKnownProductPath('/dashboards', snapshot)).toBe(true);
    expect(canAccessProductPath('/dashboards', snapshot)).toBe(false);
    expect(isKnownProductPath('/definitely-not-a-route', snapshot)).toBe(
      false,
    );
  });

  it('chooses the first allowed DB navigation destination as fallback', () => {
    const snapshot = access([
      decision('settings.license', '/settings/license', false, 'admin', 30),
      decision(
        'iam.organizations',
        '/iam/organizations',
        true,
        'admin',
        10,
      ),
      decision('traces', '/traces', true, 'investigate', 40),
    ]);
    expect(deniedProductRouteFallback('/settings/license', snapshot)).toBe(
      '/traces',
    );
  });

  it('denies everything while a new organization snapshot is loading', () => {
    const snapshot = access([decision('dashboards', '/dashboards', true)]);
    expect(
      canAccessProductPath('/dashboards', {
        ...snapshot,
        status: 'loading',
      }),
    ).toBe(false);
  });

  it('fails closed without crashing on a legacy snapshot missing routes', () => {
    const snapshot: IamCapabilitySnapshot = {
      organization_id: 'org-a',
      scope: 'organization',
      display_role: 'Viewer',
      roles: [],
      permissions: ['streams.query'],
      features: [],
      version: 3,
    };

    const legacyAccess = accessFromSnapshot(snapshot);
    expect(legacyAccess.routeCatalogVersion).toBe(0);
    expect(legacyAccess.routes).toEqual([]);
    expect(canAccessProductPath('/dashboards', legacyAccess)).toBe(false);
    expect(accessibleProductNavigation(legacyAccess, 'investigate')).toEqual(
      [],
    );
    expect(deniedProductRouteFallback('/dashboards', legacyAccess)).toBe(
      '/account/settings/profile',
    );
  });
});
