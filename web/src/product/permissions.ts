export type PermissionKey = `${string}.${string}`;
export type PermissionScope = 'platform' | 'organization';
export type PermissionDomain =
  | 'platform'
  | 'organization'
  | 'iam'
  | 'observability'
  | 'dashboards'
  | 'alerts'
  | 'pipelines'
  | 'reports'
  | 'status_pages'
  | 'synthetics'
  | 'service_levels'
  | 'postmortems'
  | 'agent';

export interface PermissionDefinition {
  key: PermissionKey;
  scope: PermissionScope;
  domain: PermissionDomain;
  labelKey: string;
  descriptionKey: string;
  builtinRoles: string[];
}

export interface PermissionCatalog {
  version: number;
  permissions: PermissionDefinition[];
}

export function isPermissionKey(
  value: string,
  catalog?: PermissionCatalog,
): value is PermissionKey {
  const normalized = value.trim().toLowerCase();
  const syntacticallyValid =
    normalized === value &&
    /^[a-z][a-z0-9_]*(?:\.[a-z][a-z0-9_]*)+$/.test(normalized);
  return (
    syntacticallyValid &&
    (!catalog ||
      catalog.permissions.some(
        (permission) => permission.key === normalized,
      ))
  );
}

export function permissionDefinition(
  catalog: PermissionCatalog | undefined,
  key: PermissionKey,
): PermissionDefinition | undefined {
  return catalog?.permissions.find((permission) => permission.key === key);
}

export function permissionDefinitionsOf(
  catalog: PermissionCatalog,
  scope: PermissionScope,
): PermissionDefinition[] {
  return catalog.permissions.filter((permission) => permission.scope === scope);
}

export function permissionDefinitionsByDomain(
  catalog: PermissionCatalog | undefined,
  scope: PermissionScope = 'organization',
): Array<{
  domain: PermissionDomain;
  permissions: PermissionDefinition[];
}> {
  const groups = new Map<PermissionDomain, PermissionDefinition[]>();
  for (const permission of catalog ? permissionDefinitionsOf(catalog, scope) : []) {
    const values = groups.get(permission.domain) ?? [];
    values.push(permission);
    groups.set(permission.domain, values);
  }
  return [...groups].map(([domain, permissions]) => ({
    domain,
    permissions,
  }));
}

/**
 * The platform permissions a grant can narrow to one organization. The others are about the whole
 * instance, so they hold across the platform or not at all.
 */
const ORGANIZATION_NARROWABLE: readonly string[] = [
  'sys.organizations.read',
  'sys.organizations.update',
  'sys.organizations.disable',
  'sys.organizations.delete',
];

export function isOrganizationNarrowable(key: string): boolean {
  return ORGANIZATION_NARROWABLE.includes(key);
}

export function normalizePermissionKeys(
  values: readonly string[] | null | undefined,
  catalog?: PermissionCatalog,
): PermissionKey[] {
  if (!values) return [];
  return [
    ...new Set(
      values
        .map((value) => value.trim().toLowerCase())
        .filter((value) => isPermissionKey(value, catalog)),
    ),
  ].sort();
}
