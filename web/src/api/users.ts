import { http } from '@/lib/http';
import type { AssignedRole } from '@/stores/auth';

export interface UserView {
  id: string;
  email: string;
  display_name: string;
  is_root: boolean;
  avatar_url?: string | null;
  disabled: boolean;
  /** "active" | "pending"（待审批的自助注册用户）。 */
  status?: string;
  display_role?: string | null;
  roles: AssignedRole[];
  team_names: string[];
  login_method: 'password' | 'oidc' | 'saml' | string;
  last_active_at_micros?: number | null;
  joined_at_micros?: number | null;
  created_at_micros: number;
}

export interface CreateUserPayload {
  email: string;
  display_name: string;
  password: string;
}

export async function list(): Promise<UserView[]> {
  const { data } = await http.get<UserView[] | { items: UserView[] }>('/users');
  return Array.isArray(data) ? data : data.items;
}

export async function get(id: string): Promise<UserView> {
  const { data } = await http.get<UserView>(`/users/${encodeURIComponent(id)}`);
  return data;
}

export async function create(payload: CreateUserPayload): Promise<UserView> {
  const { data } = await http.post<UserView>('/users', payload);
  return data;
}

export async function remove(id: string): Promise<void> {
  await http.delete(`/users/${encodeURIComponent(id)}`);
}

export interface UpsertMembershipPayload {
  user_id: string;
  role_ids: string[];
}

export async function upsertMembership(orgId: string, payload: UpsertMembershipPayload): Promise<void> {
  await http.post(`/orgs/${encodeURIComponent(orgId)}/members`, payload);
}

export async function removeMembership(orgId: string, userId: string): Promise<void> {
  await http.delete(
    `/orgs/${encodeURIComponent(orgId)}/members/${encodeURIComponent(userId)}`,
  );
}


export interface UpdateUserPayload {
  email?: string;
  display_name?: string;
  avatar_url?: string;
  disabled?: boolean;
}

export async function update(id: string, payload: UpdateUserPayload): Promise<UserView> {
  const { data } = await http.patch<UserView>(`/users/${encodeURIComponent(id)}`, payload);
  return data;
}

/** 审批自助注册的 pending 用户 → active（OrgAdmin/Owner）。 */
export async function approve(id: string): Promise<UserView> {
  const { data } = await http.post<UserView>(`/users/${encodeURIComponent(id)}/approve`, {});
  return data;
}

/** 拒绝 pending 用户 → rejected（OrgAdmin/Owner）。 */
export async function reject(id: string): Promise<UserView> {
  const { data } = await http.post<UserView>(`/users/${encodeURIComponent(id)}/reject`, {});
  return data;
}
