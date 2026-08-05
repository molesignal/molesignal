import { http } from '@/lib/http';
import type { PermissionKey } from '@/product/permissions';

export type { PermissionKey } from '@/product/permissions';

export interface RoleUsage {
  memberships: number;
  api_tokens: number;
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
  role_type: 'organization' | 'resource' | 'platform';
  scope: 'organization' | 'resource' | 'platform';
  permissions: PermissionKey[];
  usage: RoleUsage;
  created_at_micros: number;
  updated_at_micros: number;
}

export interface CreateRolePayload {
  key: string;
  name: string;
  description?: string;
  permissions: PermissionKey[];
}

export interface UpdateRolePayload {
  name: string;
  description?: string;
  permissions: PermissionKey[];
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
