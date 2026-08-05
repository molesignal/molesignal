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
  user_id: string;
  email: string;
  username?: string;
  display_name: string;
  avatar_url?: string | null;
  bio?: string;
  org_id: string;
  org_name: string;
  org_slug: string;
  display_role: string;
  created_at_micros?: number;
}

export interface UserPreferences {
  theme: PreferenceTheme;
  density: PreferenceDensity;
  language: PreferenceLanguage;
  default_home_route: string;
  time_format: PreferenceTimeFormat;
  date_format: PreferenceDateFormat;
  timezone: string;
  keyboard_shortcuts_enabled: boolean;
}

export const DEFAULT_USER_PREFERENCES: UserPreferences = {
  theme: 'system',
  density: 'normal',
  language: 'en-us',
  default_home_route: '/home',
  time_format: 'iso_24h',
  date_format: 'yyyy_mm_dd_dash',
  timezone: '',
  keyboard_shortcuts_enabled: true,
};

export async function profile(): Promise<MeProfile> {
  const { data } = await http.get<MeProfile>('/me/profile');
  return data;
}

export interface UpdateProfileInput {
  display_name?: string;
  avatar_url?: string;
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

export async function preferences(): Promise<UserPreferences> {
  const { data } = await http.get<Partial<UserPreferences>>('/me/preferences');
  return { ...DEFAULT_USER_PREFERENCES, ...data };
}

export async function updatePreferences(input: UserPreferences): Promise<UserPreferences> {
  const { data } = await http.put<UserPreferences>('/me/preferences', input);
  return { ...DEFAULT_USER_PREFERENCES, ...data };
}
