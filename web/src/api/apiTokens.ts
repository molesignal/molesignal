import { http } from '@/lib/http';

export type ApiTokenKind = 'personal' | 'default_ingestion' | 'rum_client';

export interface ApiToken {
  id: string;
  name: string;
  prefix: string;
  role_id: string;
  role_key: string;
  role_name: string;
  token_kind: ApiTokenKind;
  application_id?: string | null;
  expires_at_micros?: number | null;
  last_used_at_micros?: number | null;
  revoked: boolean;
  created_at_micros: number;
  [key: string]: unknown;
}

export interface CreateApiTokenPayload {
  name: string;
  role_id?: string | undefined;
  expires_in_days?: number | undefined;
}

export interface CreatedApiToken {
  id: string;
  prefix: string;
  token: string;
  role_id: string;
  role_key: string;
  role_name: string;
  token_kind: ApiTokenKind;
  application_id?: string | null;
  expires_at_micros?: number | null;
  created_at_micros: number;
}

export async function list(): Promise<ApiToken[]> {
  const { data } = await http.get<ApiToken[] | { items: ApiToken[] }>('/auth/tokens');
  return Array.isArray(data) ? data : data.items;
}

export async function create(payload: CreateApiTokenPayload): Promise<CreatedApiToken> {
  const { data } = await http.post<CreatedApiToken>('/auth/tokens', payload);
  return data;
}

export async function revoke(id: string): Promise<void> {
  await http.delete(`/auth/tokens/${encodeURIComponent(id)}`);
}

/**
 * The current user's default ingestion token, in full. The backend
 * auto-creates it on first call and can re-display it (its plaintext is
 * sealed at rest), so non-RUM datasource pages can show a ready-to-use token
 * without the user creating one by hand. RUM uses getRumClient instead.
 */
export async function getDefault(): Promise<CreatedApiToken> {
  const { data } = await http.get<CreatedApiToken>('/auth/tokens/default');
  return data;
}

/** Application-bound, write-only credential safe to embed in a RUM client. */
export async function getRumClient(applicationId: string): Promise<CreatedApiToken> {
  const { data } = await http.get<CreatedApiToken>('/auth/tokens/rum', {
    params: { application_id: applicationId },
  });
  return data;
}
