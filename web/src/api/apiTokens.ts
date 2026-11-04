import { http } from '@/lib/http';
import type { PermissionDefinition } from '@/product/permissions';

export type ApiTokenKind =
  | 'personal'
  | 'default_intake'
  | 'rum_client'
  | 'service_account';

/**
 * What a token may do: follow a permission policy, hold a list of permissions
 * fixed when it was issued, or, with neither, whatever its principal may do. A
 * token never does more than its principal may do at the time of a request.
 */
export interface ApiTokenScope {
  /** The permission policy the token follows, as the policy changes. */
  policyId?: string | null;
  permissions?: string[] | null;
}

/** A permission policy of the organization, as a token's scope. */
export interface TokenPolicy {
  id: string;
  key: string;
  name: string;
  description: string;
  builtin: boolean;
  permissions: string[];
}

/**
 * What a token of the organization can be limited to: its permission policies
 * and the organization permissions. Every member may read them, to name a
 * token's scope and to pick from what they hold.
 */
export interface TokenScopes {
  policies: TokenPolicy[];
  permissions: PermissionDefinition[];
}

export interface ApiToken extends ApiTokenScope {
  id: string;
  name: string;
  prefix: string;
  tokenKind: ApiTokenKind;
  /** Who issued it. A personal token acts as this user. */
  userId: string;
  applicationId?: string | null;
  serviceAccountId?: string | null;
  expiresAt?: number | null;
  lastUsedAt?: number | null;
  revoked: boolean;
  createdAt: number;
  [key: string]: unknown;
}

/** A token just issued: the one time its secret is shown. */
export interface IssuedApiToken extends ApiToken {
  token: string;
}

/**
 * A token whose secret the server shows again on request: the default intake
 * token and an application's RUM client token.
 */
export interface ManagedApiToken extends ApiTokenScope {
  id: string;
  prefix: string;
  token: string;
  tokenKind: ApiTokenKind;
  applicationId?: string | null;
  createdAt: number;
}

export interface CreateApiTokenPayload {
  name: string;
  policyId?: string | undefined;
  permissions?: string[] | undefined;
  expiresInDays?: number | undefined;
}

export interface CreateServiceAccountTokenPayload extends CreateApiTokenPayload {
  serviceAccountId: string;
}

/** Every token of the organization. */
export async function list(): Promise<ApiToken[]> {
  const { data } = await http.get<ApiToken[]>('/auth/tokens');
  return data;
}

/** A token that authenticates a service account. */
export async function createForServiceAccount(
  payload: CreateServiceAccountTokenPayload,
): Promise<IssuedApiToken> {
  const { data } = await http.post<IssuedApiToken>('/auth/tokens', payload);
  return data;
}

/** Revokes any token of the organization. */
export async function revoke(id: string): Promise<void> {
  await http.delete(`/auth/tokens/${encodeURIComponent(id)}`);
}

export async function scopes(): Promise<TokenScopes> {
  const { data } = await http.get<TokenScopes>('/me/api-tokens/scopes');
  return data;
}

/** The signed-in user's own tokens. */
export async function listMine(): Promise<ApiToken[]> {
  const { data } = await http.get<ApiToken[]>('/me/api-tokens');
  return data;
}

export async function createMine(payload: CreateApiTokenPayload): Promise<IssuedApiToken> {
  const { data } = await http.post<IssuedApiToken>('/me/api-tokens', payload);
  return data;
}

export async function revokeMine(id: string): Promise<void> {
  await http.delete(`/me/api-tokens/${encodeURIComponent(id)}`);
}

/**
 * The signed-in user's default intake token, in full. The backend creates it
 * on first call and shows it again later (its secret is sealed at rest), so a
 * non-RUM data source page can offer a ready token. It only writes data. RUM
 * uses getRumClient instead.
 */
export async function getDefault(): Promise<ManagedApiToken> {
  const { data } = await http.get<ManagedApiToken>('/me/api-tokens/default');
  return data;
}

/** Application-bound, write-only credential safe to embed in a RUM client. */
export async function getRumClient(applicationId: string): Promise<ManagedApiToken> {
  const { data } = await http.get<ManagedApiToken>('/auth/tokens/rum', {
    params: { applicationId: applicationId },
  });
  return data;
}
