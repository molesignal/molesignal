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
import type { PermissionKey, PermissionScope } from '@/product/permissions';
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
  /** Platform permissions held on named organizations only: permission to organization ids. */
  organizationPermissions?: Readonly<Record<string, readonly string[]>>;
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
  const context = useAuthStore((state) => state.ctx);
  const query = useQuery({
    queryKey: capabilityQueryKey(context?.orgId ?? ''),
    queryFn: async () => {
      const snapshot = await iamApi.capabilities();
      if (snapshot.organizationId !== useAuthStore.getState().ctx?.orgId) {
        throw new Error('capability snapshot organization mismatch');
      }
      return snapshot;
    },
    enabled: Boolean(context),
    staleTime: 0,
    refetchOnWindowFocus: true,
    retry: false,
  });

  return useMemo(() => {
    if (!context) return null;
    if (!query.data) {
      return {
        organizationId: context.orgId,
        role: normalizeRole(context.displayRole),
        scope: normalizeAuthScope(context.scope),
        permissions: new Set<PermissionKey>(),
        version: 0,
        routeCatalogVersion: 0,
        routes: [],
        status: query.isError ? 'error' : 'loading',
      };
    }
    return accessFromSnapshot(query.data);
  }, [context, query.data, query.isError]);
}

export function accessFromSnapshot(
  snapshot: iamApi.IamCapabilitySnapshot,
): ProductAccess {
  return {
    organizationId: snapshot.organizationId,
    role: normalizeRole(snapshot.displayRole),
    scope: normalizeAuthScope(snapshot.scope),
    permissions: new Set(snapshot.permissions),
    organizationPermissions: snapshot.organizationPermissions ?? {},
    version: snapshot.version,
    routeCatalogVersion: snapshot.routeCatalogVersion ?? 0,
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
 * Whether the catalog closes this path to the reader. A path it says nothing about is left to the
 * route guard: this decides what to offer, not what to allow.
 */
export function isProductPathDenied(
  pathname: string,
  access: ProductAccess | null,
): boolean {
  return (
    access?.status === 'ready' &&
    findRouteDecision(pathname, routeDecisions(access))?.allowed === false
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

/** Whether the permission reaches this organization: across the platform, or by name. */
export function reachesOrganization(
  permission: PermissionKey,
  organizationId: string,
  access: ProductAccess | null,
): boolean {
  return (
    hasPermission(permission, access) ||
    (access?.status === 'ready' &&
      (access.organizationPermissions?.[permission]?.includes(organizationId) ?? false))
  );
}

/** The permissions that count in this session: the platform's in the platform organization. */
export function useSessionPermissionScope(): PermissionScope {
  return useProductAccess()?.scope === 'system' ? 'platform' : 'organization';
}

export function accessibleProductNavigation(
  access: ProductAccess | null,
  group: ProductNavGroup,
): ProductRouteMeta[] {
  if (access?.status !== 'ready') return [];
  return routeDecisions(access)
    .filter(
      (decision) =>
        decision.allowed && decision.navigationGroup === group,
    )
    .sort(
      (left, right) =>
        (left.navigationPosition ?? Number.MAX_SAFE_INTEGER) -
          (right.navigationPosition ?? Number.MAX_SAFE_INTEGER) ||
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
        decision.allowed && decision.navigationGroup !== undefined,
    )
    .sort(navigationDecisionOrder)
    .map((decision) => getProductRouteById(decision.id)?.path)
    .filter(
      (candidate): candidate is `/${string}` =>
        Boolean(candidate && !candidate.includes(':') && !candidate.includes('*')),
    );
  const anyAllowedStaticRoute = decisions
    .filter((decision) => decision.allowed)
    .map((decision) => decision.pathPattern)
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
    left.navigationGroup as ProductNavGroup,
  );
  const rightGroup = PRODUCT_NAV_GROUPS.indexOf(
    right.navigationGroup as ProductNavGroup,
  );
  return (
    (leftGroup < 0 ? Number.MAX_SAFE_INTEGER : leftGroup) -
      (rightGroup < 0 ? Number.MAX_SAFE_INTEGER : rightGroup) ||
    (left.navigationPosition ?? Number.MAX_SAFE_INTEGER) -
      (right.navigationPosition ?? Number.MAX_SAFE_INTEGER) ||
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
    .filter((decision) => routePatternMatches(decision.pathPattern, pathname))
    .sort(
      (left, right) =>
        routeSpecificity(right.pathPattern) -
          routeSpecificity(left.pathPattern) ||
        right.pathPattern.length - left.pathPattern.length,
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
