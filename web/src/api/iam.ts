import { http } from '@/lib/http';
import type { PermissionCatalog, PermissionKey } from '@/product/permissions';
import type { AssignedRole, AuthScope } from '@/stores/auth';

export interface IamCapabilitySnapshot {
  organization_id: string;
  scope: AuthScope;
  display_role: string;
  roles: AssignedRole[];
  permissions: PermissionKey[];
  features: string[];
  version: number;
  /** Absent only on a rolling upgrade from a pre-route-catalog backend. */
  route_catalog_version?: number;
  /** Missing legacy snapshots fail closed until the backend is upgraded. */
  routes?: IamRouteAccess[];
}

export interface IamRouteAccess {
  id: string;
  path_pattern: string;
  allowed: boolean;
  navigation_group?: 'home' | 'investigate' | 'pipeline' | 'admin';
  navigation_position?: number;
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
  organization_id: string;
  role_id: string;
  principal_type: PrincipalType;
  principal_id: string;
  resource_type?: string;
  resource_id?: string;
  conditions: Record<string, unknown>;
  starts_at?: number;
  expires_at?: number;
  created_by: string;
  created_at: number;
}

export interface ResourceRelationship {
  id: string;
  organization_id: string;
  resource_type: string;
  resource_id: string;
  role_id: string;
  subject_type: PrincipalType;
  subject_id: string;
  container_type?: string;
  container_id?: string;
  created_by: string;
  created_at: number;
}

export interface CrossOrgGrant {
  id: string;
  source_organization_id: string;
  target_organization_id: string;
  grantee_type: PrincipalType;
  grantee_id: string;
  resource_type: string;
  resource_selector: { ids?: string[]; all?: boolean };
  permissions: PermissionKey[];
  conditions: Record<string, unknown>;
  starts_at?: number;
  expires_at?: number;
  status: CrossOrgGrantStatus;
  approved_by?: string;
  approved_at?: number;
  revoked_by?: string;
  revoked_at?: number;
  created_by: string;
  created_at: number;
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
  role_id: string;
  principal_type: PrincipalType;
  principal_id: string;
  resource_type?: string;
  resource_id?: string;
  conditions?: Record<string, unknown>;
  starts_at_micros?: number;
  expires_at_micros?: number;
}

export interface IamTarget {
  organization_id?: string;
  resource_type: string;
  resource_id: string;
  container_type?: string;
  container_id?: string;
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
  policy_version: number;
  matched_binding_ids?: string[];
  matched_relationship_ids?: string[];
  matched_grant_ids?: string[];
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
    'id' | 'organization_id' | 'created_by' | 'created_at'
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
    | 'target_organization_id'
    | 'grantee_type'
    | 'grantee_id'
    | 'resource_type'
    | 'resource_selector'
    | 'permissions'
    | 'conditions'
  > & {
    starts_at_micros?: number;
    expires_at_micros?: number;
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
