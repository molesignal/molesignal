import { http } from '@/lib/http';
import type { PermissionKey } from '@/product/permissions';

export type { PermissionKey } from '@/product/permissions';

export interface RoleUsage {
  memberships: number;
  serviceAccounts: number;
  invitations: number;
  bindings: number;
  total: number;
}

export interface IamRole {
  id: string;
  key: string;
  name: string;
  description: string;
  builtin: boolean;
  roleType: 'organization' | 'resource' | 'platform';
  scope: 'organization' | 'resource' | 'platform';
  /** The role's own permissions. */
  permissions: PermissionKey[];
  /** The permission policies it attaches. */
  policyIds: string[];
  /** Its own permissions and those of its policies: what it grants. */
  effectivePermissions: PermissionKey[];
  usage: RoleUsage;
  createdAt: number;
  updatedAt: number;
}

export interface CreateRolePayload {
  key: string;
  name: string;
  description?: string;
  permissions: PermissionKey[];
  policyIds: string[];
}

export interface UpdateRolePayload {
  name: string;
  description?: string;
  permissions: PermissionKey[];
  policyIds: string[];
}

export async function list(): Promise<IamRole[]> {
  const { data } = await http.get<IamRole[]>('/roles');
  return data;
}

export async function create(payload: CreateRolePayload): Promise<IamRole> {
  const { data } = await http.post<IamRole>('/roles', payload);
  return data;
}

export async function update(id: string, payload: UpdateRolePayload): Promise<IamRole> {
  const { data } = await http.patch<IamRole>(`/roles/${encodeURIComponent(id)}`, payload);
  return data;
}

export async function remove(id: string): Promise<void> {
  await http.delete(`/roles/${encodeURIComponent(id)}`);
}
