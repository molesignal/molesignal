import { useQuery } from '@tanstack/react-query';
import { useMemo } from 'react';

import * as iamApi from '@/api/iam';
import {
  findProductRoute,
  getProductRouteById,
  PRODUCT_NAV_GROUPS,
  type ProductNavGroup,
  type ProductRouteMeta,
} from '@/product/ia';
import type { PermissionKey } from '@/product/permissions';
import {
  normalizeAuthScope,
  normalizeRole,
  type AuthScope,
  type Role,
  useAuthStore,
} from '@/stores/auth';

export type ProductAccessStatus = 'loading' | 'ready' | 'error';

export interface ProductAccess {
  organizationId: string;
  role: Role;
  scope: AuthScope;
  permissions: ReadonlySet<PermissionKey>;
  features: ReadonlySet<string>;
  version: number;
  routeCatalogVersion: number;
  routes: readonly iamApi.IamRouteAccess[];
  status: ProductAccessStatus;
}

export const capabilityQueryKey = (organizationId: string) =>
  ['iam', 'capabilities', organizationId] as const;

const EMPTY_ROUTE_DECISIONS: readonly iamApi.IamRouteAccess[] = [];

/**
 * Capability bootstrap. The query key includes the signed active organization
 * and no previous snapshot is used as placeholder data, so an organization
 * switch cannot render routes from the old workspace.
 */
export function useProductAccess(): ProductAccess | null {
  const token = useAuthStore((state) => state.token);
  const context = useAuthStore((state) => state.ctx);
  const query = useQuery({
    queryKey: capabilityQueryKey(context?.org_id ?? ''),
    queryFn: async () => {
      const snapshot = await iamApi.capabilities();
      if (snapshot.organization_id !== useAuthStore.getState().ctx?.org_id) {
        throw new Error('capability snapshot organization mismatch');
      }
      return snapshot;
    },
    enabled: Boolean(token && context),
    staleTime: 0,
    refetchOnWindowFocus: true,
    retry: false,
  });

  return useMemo(() => {
    if (!context || !token) return null;
    if (!query.data) {
      return {
        organizationId: context.org_id,
        role: normalizeRole(context.display_role),
        scope: normalizeAuthScope(context.scope),
        permissions: new Set<PermissionKey>(),
        features: new Set<string>(),
        version: 0,
        routeCatalogVersion: 0,
        routes: [],
        status: query.isError ? 'error' : 'loading',
      };
    }
    return accessFromSnapshot(query.data);
  }, [context, query.data, query.isError, token]);
}

export function accessFromSnapshot(
  snapshot: iamApi.IamCapabilitySnapshot,
): ProductAccess {
  return {
    organizationId: snapshot.organization_id,
    role: normalizeRole(snapshot.display_role),
    scope: normalizeAuthScope(snapshot.scope),
    permissions: new Set(snapshot.permissions),
    features: new Set(snapshot.features),
    version: snapshot.version,
    routeCatalogVersion: snapshot.route_catalog_version ?? 0,
    routes: Array.isArray(snapshot.routes)
      ? snapshot.routes
      : EMPTY_ROUTE_DECISIONS,
    status: 'ready',
  };
}

/** Route authorization is resolved by the backend from the DB catalog. */
export function canAccessProductRoute(
  route: ProductRouteMeta,
  access: ProductAccess | null,
): boolean {
  if (access?.status !== 'ready') return false;
  const decisions = routeDecisions(access);
  return (
    decisions.find((decision) => decision.id === route.id)?.allowed ??
    findRouteDecision(route.path, decisions)?.allowed ??
    false
  );
}

export function canAccessProductPath(
  pathname: string,
  access: ProductAccess | null,
): boolean {
  return (
    access?.status === 'ready' &&
    (findRouteDecision(pathname, routeDecisions(access))?.allowed ?? false)
  );
}

/**
 * Unknown paths continue to the router's wildcard 404. A registered frontend
 * route without a database decision is known but denied, which fails closed
 * when the two catalogs drift.
 */
export function isKnownProductPath(
  pathname: string,
  access: ProductAccess | null,
): boolean {
  return Boolean(
    findProductRoute(pathname) ||
      (access?.status === 'ready' &&
        findRouteDecision(pathname, routeDecisions(access))),
  );
}

export function hasPermission(
  permission: PermissionKey,
  access: ProductAccess | null,
): boolean {
  return access?.status === 'ready' && access.permissions.has(permission);
}

export function accessibleProductNavigation(
  access: ProductAccess | null,
  group: ProductNavGroup,
): ProductRouteMeta[] {
  if (access?.status !== 'ready') return [];
  return routeDecisions(access)
    .filter(
      (decision) =>
        decision.allowed && decision.navigation_group === group,
    )
    .sort(
      (left, right) =>
        (left.navigation_position ?? Number.MAX_SAFE_INTEGER) -
          (right.navigation_position ?? Number.MAX_SAFE_INTEGER) ||
        left.id.localeCompare(right.id),
    )
    .map((decision) => getProductRouteById(decision.id))
    .filter((route): route is ProductRouteMeta => Boolean(route));
}

export function deniedProductRouteFallback(
  pathname: string,
  access: ProductAccess | null,
): string {
  if (access?.status !== 'ready') return '/account/settings/profile';
  const decisions = routeDecisions(access);
  const navigationCandidates = decisions
    .filter(
      (decision) =>
        decision.allowed && decision.navigation_group !== undefined,
    )
    .sort(navigationDecisionOrder)
    .map((decision) => getProductRouteById(decision.id)?.path)
    .filter(
      (candidate): candidate is `/${string}` =>
        Boolean(candidate && !candidate.includes(':') && !candidate.includes('*')),
    );
  const anyAllowedStaticRoute = decisions
    .filter((decision) => decision.allowed)
    .map((decision) => decision.path_pattern)
    .find(
      (candidate) =>
        candidate !== pathname &&
        !candidate.includes(':') &&
        !candidate.includes('*'),
    );
  return (
    navigationCandidates.find((candidate) => candidate !== pathname) ??
    anyAllowedStaticRoute ??
    '/account/settings/profile'
  );
}

function routeDecisions(
  access: ProductAccess | null,
): readonly iamApi.IamRouteAccess[] {
  return Array.isArray(access?.routes) ? access.routes : EMPTY_ROUTE_DECISIONS;
}

function navigationDecisionOrder(
  left: iamApi.IamRouteAccess,
  right: iamApi.IamRouteAccess,
): number {
  const leftGroup = PRODUCT_NAV_GROUPS.indexOf(
    left.navigation_group as ProductNavGroup,
  );
  const rightGroup = PRODUCT_NAV_GROUPS.indexOf(
    right.navigation_group as ProductNavGroup,
  );
  return (
    (leftGroup < 0 ? Number.MAX_SAFE_INTEGER : leftGroup) -
      (rightGroup < 0 ? Number.MAX_SAFE_INTEGER : rightGroup) ||
    (left.navigation_position ?? Number.MAX_SAFE_INTEGER) -
      (right.navigation_position ?? Number.MAX_SAFE_INTEGER) ||
    left.id.localeCompare(right.id)
  );
}

export function routePatternMatches(
  pathPattern: string,
  pathname: string,
): boolean {
  const patternSegments = splitPath(pathPattern);
  const pathSegments = splitPath(pathname);
  for (let index = 0; index < patternSegments.length; index += 1) {
    const patternSegment = patternSegments[index];
    if (patternSegment === '*') return true;
    if (index >= pathSegments.length) return false;
    if (
      !patternSegment?.startsWith(':') &&
      patternSegment !== pathSegments[index]
    ) {
      return false;
    }
  }
  return patternSegments.length === pathSegments.length;
}

function findRouteDecision(
  pathname: string,
  decisions: readonly iamApi.IamRouteAccess[],
): iamApi.IamRouteAccess | undefined {
  return decisions
    .filter((decision) => routePatternMatches(decision.path_pattern, pathname))
    .sort(
      (left, right) =>
        routeSpecificity(right.path_pattern) -
          routeSpecificity(left.path_pattern) ||
        right.path_pattern.length - left.path_pattern.length,
    )[0];
}

function routeSpecificity(pathPattern: string): number {
  return splitPath(pathPattern).reduce((score, segment) => {
    if (segment === '*') return score;
    if (segment.startsWith(':')) return score + 10;
    return score + 100;
  }, 0);
}

function splitPath(path: string): string[] {
  const normalized = path.split(/[?#]/, 1)[0]?.replace(/^\/+|\/+$/g, '') ?? '';
  return normalized ? normalized.split('/') : [];
}
