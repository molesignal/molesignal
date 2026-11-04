import { http } from '@/lib/http';

export type PreferenceTheme = 'system' | 'dark' | 'light';
export type PreferenceDensity = 'compact' | 'normal' | 'comfortable';
export type PreferenceLanguage = 'en-us' | 'zh-cn';
export type PreferenceTimeFormat = 'iso_24h' | 'local_12h';
export type PreferenceDateFormat =
  | 'yyyy_mm_dd_dash'
  | 'yyyy_mm_dd_slash'
  | 'dd_mm_yyyy_slash'
  | 'mm_dd_yyyy_slash';

export interface MeProfile {
  userId: string;
  email: string;
  username?: string;
  displayName: string;
  avatarUrl?: string | null;
  bio?: string;
  orgId: string;
  orgName: string;
  orgSlug: string;
  displayRole: string;
  createdAt?: number;
}

export interface UserPreferences {
  theme: PreferenceTheme;
  density: PreferenceDensity;
  language: PreferenceLanguage;
  defaultHomeRoute: string;
  timeFormat: PreferenceTimeFormat;
  dateFormat: PreferenceDateFormat;
  timezone: string;
  keyboardShortcutsEnabled: boolean;
}

export const DEFAULT_USER_PREFERENCES: UserPreferences = {
  theme: 'system',
  density: 'normal',
  language: 'en-us',
  defaultHomeRoute: '/home',
  timeFormat: 'iso_24h',
  dateFormat: 'yyyy_mm_dd_dash',
  timezone: '',
  keyboardShortcutsEnabled: true,
};

export async function profile(): Promise<MeProfile> {
  const { data } = await http.get<MeProfile>('/me/profile');
  return data;
}

export interface UpdateProfileInput {
  displayName?: string;
  avatarUrl?: string;
  bio?: string;
}

export async function updateProfile(input: UpdateProfileInput): Promise<MeProfile> {
  const { data } = await http.put<MeProfile>('/me/profile', input);
  return data;
}

/** Upload an avatar image (raw bytes) to object storage; returns the updated profile. */
export async function uploadAvatar(file: File): Promise<MeProfile> {
  const { data } = await http.post<MeProfile>('/me/avatar', file, {
    headers: { 'Content-Type': file.type || 'application/octet-stream' },
  });
  return data;
}

/** How the user prefers a page to look, as the server keeps it. `null` where it keeps nothing. */
export interface UiPreferenceEntry {
  value: unknown;
  /** Microseconds since epoch. */
  updatedAt: number | null;
}

const uiPreferencePath = (scope: string, key: string) =>
  `/me/ui-preference/${encodeURIComponent(scope)}/${encodeURIComponent(key)}`;

export async function getUiPreference(scope: string, key: string): Promise<UiPreferenceEntry> {
  const { data } = await http.get<Partial<UiPreferenceEntry>>(uiPreferencePath(scope, key));
  return { value: data.value ?? null, updatedAt: data.updatedAt ?? null };
}

export async function putUiPreference(scope: string, key: string, value: unknown): Promise<UiPreferenceEntry> {
  const { data } = await http.put<Partial<UiPreferenceEntry>>(uiPreferencePath(scope, key), { value });
  return { value: data.value ?? null, updatedAt: data.updatedAt ?? null };
}

export async function deleteUiPreference(scope: string, key: string): Promise<void> {
  await http.delete(uiPreferencePath(scope, key));
}

export async function preferences(): Promise<UserPreferences> {
  const { data } = await http.get<Partial<UserPreferences>>('/me/preferences');
  return { ...DEFAULT_USER_PREFERENCES, ...data };
}

export async function updatePreferences(input: UserPreferences): Promise<UserPreferences> {
  const { data } = await http.put<UserPreferences>('/me/preferences', input);
  return { ...DEFAULT_USER_PREFERENCES, ...data };
}
