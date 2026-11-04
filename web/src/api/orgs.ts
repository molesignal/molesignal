import type { MfaMethod } from '@/api/auth';
import {
  DEFAULT_USER_PREFERENCES,
  type UserPreferences,
} from '@/api/me';
import type { Proof } from '@/api/mfa';
import type { PublicSsoProvider } from '@/api/sso';
import { http } from '@/lib/http';
import type { AssignedRole } from '@/stores/auth';

export interface Org {
  id: string;
  name: string;
  slug?: string;
  displayRole?: string | null;
  roles: AssignedRole[];
  system?: boolean;
  disabled: boolean;
  /**
   * Whether the caller can enter the organization. From the platform
   * organization an owner administers organizations they do not belong to:
   * those are listed and cannot be entered. Absent on an older backend, where
   * everything listed could be.
   */
  accessible?: boolean;
}

/** The organization entered; the session cookies are rewritten for it. */
export interface SelectOrgResponse {
  userId: string;
  orgId: string;
  orgName?: string;
  displayRole: string;
  roles: AssignedRole[];
  system: boolean;
}

/**
 * What the organization's sign-in requirements still ask of this sign-in: a second
 * factor on the spot (sent back with the request), setting one up first, or signing
 * in through the organization's own single sign-on.
 */
export type OrgEntryWaiting =
  | { mfa: { organizationName: string; methods: MfaMethod[] } }
  | { mfaSetup: { organizationName: string } }
  | { sso: { organizationName: string; providers: PublicSsoProvider[] } };

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

export async function selectOrg(
  id: string,
  proof?: Proof,
): Promise<SelectOrgResponse | OrgEntryWaiting> {
  const { data } = await http.post<SelectOrgResponse | OrgEntryWaiting>(
    `/orgs/${encodeURIComponent(id)}/select`,
    proof ? { proof } : undefined,
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
