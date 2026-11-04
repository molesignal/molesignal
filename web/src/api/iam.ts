import { http } from '@/lib/http';
import type { PermissionCatalog, PermissionKey } from '@/product/permissions';
import type { AssignedRole, AuthScope } from '@/stores/auth';

export interface IamCapabilitySnapshot {
  organizationId: string;
  scope: AuthScope;
  displayRole: string;
  roles: AssignedRole[];
  permissions: PermissionKey[];
  /**
   * Platform permissions held on named organizations only: permission to organization ids.
   * `permissions` is what is held without any such limit.
   */
  organizationPermissions?: Record<string, string[]>;
  version: number;
  /** Absent only on a rolling upgrade from a pre-route-catalog backend. */
  routeCatalogVersion?: number;
  /** Missing legacy snapshots fail closed until the backend is upgraded. */
  routes?: IamRouteAccess[];
}

export interface IamRouteAccess {
  id: string;
  pathPattern: string;
  allowed: boolean;
  navigationGroup?:
    | 'home'
    | 'observe'
    | 'reliability'
    | 'ai_ops'
    | 'data'
    | 'collaboration'
    | 'admin';
  navigationPosition?: number;
}

export type PrincipalType =
  | 'user'
  | 'team'
  | 'group'
  | 'service_account'
  | 'organization';
export type CrossOrgGrantStatus = 'pending' | 'active' | 'revoked';

export interface RoleBinding {
  id: string;
  organizationId: string;
  roleId: string;
  principalType: PrincipalType;
  principalId: string;
  resourceType?: string;
  resourceId?: string;
  conditions: Record<string, unknown>;
  startsAt?: number;
  expiresAt?: number;
  createdBy: string;
  createdAt: number;
}

export interface ResourceRelationship {
  id: string;
  organizationId: string;
  resourceType: string;
  resourceId: string;
  roleId: string;
  subjectType: PrincipalType;
  subjectId: string;
  containerType?: string;
  containerId?: string;
  createdBy: string;
  createdAt: number;
}

export interface CrossOrgGrant {
  id: string;
  sourceOrganizationId: string;
  targetOrganizationId: string;
  granteeType: PrincipalType;
  granteeId: string;
  resourceType: string;
  resourceSelector: { ids?: string[]; all?: boolean };
  permissions: PermissionKey[];
  conditions: Record<string, unknown>;
  startsAt?: number;
  expiresAt?: number;
  status: CrossOrgGrantStatus;
  approvedBy?: string;
  approvedAt?: number;
  revokedBy?: string;
  revokedAt?: number;
  createdBy: string;
  createdAt: number;
}

export interface MutationResponse<T> {
  value: T;
  version: number;
}

export interface IamShareTarget {
  id: string;
  name: string;
}

export interface CreateRoleBindingPayload {
  roleId: string;
  principalType: PrincipalType;
  principalId: string;
  resourceType?: string;
  resourceId?: string;
  conditions?: Record<string, unknown>;
  startsAt?: number;
  expiresAt?: number;
}

export interface IamTarget {
  organizationId?: string;
  resourceType: string;
  resourceId: string;
  containerType?: string;
  containerId?: string;
}

export interface IamAccessRequest {
  permission: PermissionKey;
  target?: IamTarget;
  attributes?: {
    environment?: string;
    labels?: Record<string, string>;
  };
}

export interface IamDecision {
  allowed: boolean;
  reason: string;
  policyVersion: number;
  matchedBindingIds?: string[];
  matchedRelationshipIds?: string[];
  matchedGrantIds?: string[];
}

/** An account as a platform grant names it. */
export interface PlatformAccount {
  id: string;
  email: string;
  displayName: string;
  disabled: boolean;
}

/** Accounts whose email or name holds the text (`sys.administrators.read`). */
export async function searchPlatformAccounts(q: string): Promise<PlatformAccount[]> {
  const { data } = await http.get<PlatformAccount[]>('/iam/platform/accounts', { params: { q } });
  return data;
}

/** The accounts a list of platform grants names (`sys.administrators.read`). */
export async function platformAccountsByIds(ids: readonly string[]): Promise<PlatformAccount[]> {
  const { data } = await http.get<PlatformAccount[]>('/iam/platform/accounts', {
    params: { ids: ids.join(',') },
  });
  return data;
}

export async function capabilities(): Promise<IamCapabilitySnapshot> {
  const { data } = await http.get<IamCapabilitySnapshot>('/iam/capabilities');
  return data;
}

export async function permissionCatalog(): Promise<PermissionCatalog> {
  const { data } = await http.get<PermissionCatalog>('/iam/permissions');
  return data;
}

export async function listShareTargets(): Promise<IamShareTarget[]> {
  const { data } = await http.get<IamShareTarget[]>('/iam/share-targets');
  return data;
}

export async function listRoleBindings(): Promise<RoleBinding[]> {
  const { data } = await http.get<RoleBinding[]>('/iam/role-bindings');
  return data;
}

export async function createRoleBinding(
  payload: CreateRoleBindingPayload,
): Promise<MutationResponse<RoleBinding>> {
  const { data } = await http.post<MutationResponse<RoleBinding>>(
    '/iam/role-bindings',
    payload,
  );
  return data;
}

export async function removeRoleBinding(id: string): Promise<void> {
  await http.delete(`/iam/role-bindings/${encodeURIComponent(id)}`);
}

export async function listRelationships(): Promise<ResourceRelationship[]> {
  const { data } = await http.get<ResourceRelationship[]>('/iam/relationships');
  return data;
}

export async function createRelationship(
  payload: Omit<
    ResourceRelationship,
    'id' | 'organizationId' | 'createdBy' | 'createdAt'
  >,
): Promise<MutationResponse<ResourceRelationship>> {
  const { data } = await http.post<MutationResponse<ResourceRelationship>>(
    '/iam/relationships',
    payload,
  );
  return data;
}

export async function removeRelationship(id: string): Promise<void> {
  await http.delete(`/iam/relationships/${encodeURIComponent(id)}`);
}

export async function listCrossOrgGrants(): Promise<CrossOrgGrant[]> {
  const { data } = await http.get<CrossOrgGrant[]>('/iam/cross-org-grants');
  return data;
}

export async function createCrossOrgGrant(
  payload: Pick<
    CrossOrgGrant,
    | 'targetOrganizationId'
    | 'granteeType'
    | 'granteeId'
    | 'resourceType'
    | 'resourceSelector'
    | 'permissions'
    | 'conditions'
  > & {
    startsAt?: number;
    expiresAt?: number;
  },
): Promise<MutationResponse<CrossOrgGrant>> {
  const { data } = await http.post<MutationResponse<CrossOrgGrant>>(
    '/iam/cross-org-grants',
    payload,
  );
  return data;
}

export async function acceptCrossOrgGrant(
  id: string,
): Promise<MutationResponse<CrossOrgGrant>> {
  const { data } = await http.post<MutationResponse<CrossOrgGrant>>(
    `/iam/cross-org-grants/${encodeURIComponent(id)}/accept`,
  );
  return data;
}

export async function revokeCrossOrgGrant(
  id: string,
): Promise<MutationResponse<CrossOrgGrant>> {
  const { data } = await http.post<MutationResponse<CrossOrgGrant>>(
    `/iam/cross-org-grants/${encodeURIComponent(id)}/revoke`,
  );
  return data;
}

export async function evaluateBatch(
  requests: IamAccessRequest[],
): Promise<IamDecision[]> {
  const { data } = await http.post<{ decisions: IamDecision[] }>(
    '/iam/evaluate-batch',
    { requests },
  );
  return data.decisions;
}
