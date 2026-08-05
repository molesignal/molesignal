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
  | 'intelligence';

export interface PermissionDefinition {
  key: PermissionKey;
  scope: PermissionScope;
  domain: PermissionDomain;
  label_key: string;
  description_key: string;
  builtin_roles: string[];
  feature?: string;
}

export interface PermissionBundle {
  key: string;
  label_key: string;
  description_key: string;
  permissions: PermissionKey[];
}

export interface PermissionCatalog {
  version: number;
  permissions: PermissionDefinition[];
  bundles: PermissionBundle[];
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

export function organizationPermissionDefinitions(
  catalog: PermissionCatalog,
): PermissionDefinition[] {
  return catalog.permissions.filter(
    (permission) => permission.scope === 'organization',
  );
}

export function permissionDefinitionsByDomain(
  catalog: PermissionCatalog | undefined,
): Array<{
  domain: PermissionDomain;
  permissions: PermissionDefinition[];
}> {
  const groups = new Map<PermissionDomain, PermissionDefinition[]>();
  for (const permission of catalog
    ? organizationPermissionDefinitions(catalog)
    : []) {
    const values = groups.get(permission.domain) ?? [];
    values.push(permission);
    groups.set(permission.domain, values);
  }
  return [...groups].map(([domain, permissions]) => ({
    domain,
    permissions,
  }));
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
