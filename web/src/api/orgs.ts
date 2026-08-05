import {
  DEFAULT_USER_PREFERENCES,
  type UserPreferences,
} from '@/api/me';
import { http } from '@/lib/http';
import type { AssignedRole } from '@/stores/auth';

export interface Org {
  id: string;
  name: string;
  slug?: string;
  display_role?: string | null;
  roles: AssignedRole[];
  system?: boolean;
  disabled: boolean;
}

interface SelectOrgResponse {
  token: string;
  user_id: string;
  org_id: string;
  org_name?: string;
  display_role: string;
  roles: AssignedRole[];
  system: boolean;
}

export async function listOrgs(): Promise<Org[]> {
  const { data } = await http.get<Org[] | { items: Org[] }>('/orgs');
  return Array.isArray(data) ? data : data.items;
}

export interface CreateOrgInput {
  name: string;
  slug: string;
}

export async function createOrg(input: CreateOrgInput): Promise<Org> {
  const { data } = await http.post<Org>('/orgs', input);
  return data;
}

export async function selectOrg(id: string): Promise<SelectOrgResponse> {
  const { data } = await http.post<SelectOrgResponse>(
    `/orgs/${encodeURIComponent(id)}/select`,
  );
  return data;
}


export interface UpdateOrgInput {
  name?: string;
}

export async function updateOrg(id: string, input: UpdateOrgInput): Promise<Org> {
  const { data } = await http.patch<Org>(`/orgs/${encodeURIComponent(id)}`, input);
  return data;
}

export async function setOrgDisabled(id: string, disabled: boolean): Promise<Org> {
  const { data } = await http.patch<Org>(
    `/orgs/${encodeURIComponent(id)}/status`,
    { disabled },
  );
  return data;
}

export async function removeOrg(id: string): Promise<void> {
  await http.delete(`/orgs/${encodeURIComponent(id)}`);
}

export async function preferenceDefaults(): Promise<UserPreferences> {
  const { data } = await http.get<Partial<UserPreferences>>(
    '/workspace/preferences',
  );
  return { ...DEFAULT_USER_PREFERENCES, ...data };
}

export async function updatePreferenceDefaults(
  input: UserPreferences,
): Promise<UserPreferences> {
  const { data } = await http.put<UserPreferences>(
    '/workspace/preferences',
    input,
  );
  return { ...DEFAULT_USER_PREFERENCES, ...data };
}
