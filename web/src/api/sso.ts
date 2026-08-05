import { http } from '@/lib/http';

import type { SigninResponse } from './auth';

export type SsoProviderKind = 'oidc' | 'saml' | 'ldap';

type RoleMapping = Record<string, string>;

export interface SsoFieldMapping {
  subject: string;
  email: string;
  display_name: string;
  groups: string;
}

export interface OidcProviderConfig {
  issuer: string;
  authorize_url: string;
  token_url: string;
  userinfo_url?: string;
  discovery_url?: string;
  jwks_uri?: string;
  client_id: string;
  client_secret?: string;
  has_client_secret?: boolean;
  redirect_uri: string;
  scopes: string[];
  field_mapping: SsoFieldMapping;
  group_role_mapping: RoleMapping;
  default_role_id?: string;
}

export interface SamlProviderConfig {
  sp_entity_id: string;
  idp_entity_id: string;
  idp_sso_url: string;
  idp_x509_cert: string;
  assertion_consumer_url: string;
  field_mapping: SsoFieldMapping;
  group_role_mapping: RoleMapping;
  default_role_id?: string;
}

export interface LdapProviderConfig {
  url: string;
  start_tls: boolean;
  bind_dn: string;
  bind_password?: string;
  has_bind_password?: boolean;
  base_dn: string;
  user_filter: string;
  field_mapping: SsoFieldMapping;
  group_role_mapping: RoleMapping;
  default_role_id?: string;
}

export interface PublicSsoProvider {
  id: string;
  name: string;
  kind: SsoProviderKind;
}

export interface SsoAssignableRole {
  id: string;
  name: string;
}

export interface SsoProvider extends PublicSsoProvider {
  org_id: string;
  enabled: boolean;
  oidc?: OidcProviderConfig;
  saml?: SamlProviderConfig;
  ldap?: LdapProviderConfig;
  created_at_micros?: number;
  updated_at_micros?: number;
}

export interface SsoProviderInput {
  name: string;
  kind: SsoProviderKind;
  enabled?: boolean;
  oidc?: OidcProviderConfig;
  saml?: SamlProviderConfig;
  ldap?: LdapProviderConfig;
}

export async function listPublic(): Promise<PublicSsoProvider[]> {
  const { data } = await http.get<PublicSsoProvider[]>('/auth/sso/providers');
  return data;
}

export async function list(): Promise<SsoProvider[]> {
  const { data } = await http.get<SsoProvider[]>('/sso/providers');
  return data;
}

export async function listAssignableRoles(): Promise<SsoAssignableRole[]> {
  const { data } = await http.get<SsoAssignableRole[]>(
    '/sso/providers/roles',
  );
  return data;
}

export async function get(id: string): Promise<SsoProvider> {
  const { data } = await http.get<SsoProvider>(`/sso/providers/${encodeURIComponent(id)}`);
  return data;
}

export async function create(payload: SsoProviderInput): Promise<SsoProvider> {
  const { data } = await http.post<SsoProvider>('/sso/providers', payload);
  return data;
}

export async function update(id: string, payload: SsoProviderInput): Promise<SsoProvider> {
  const { data } = await http.put<SsoProvider>(
    `/sso/providers/${encodeURIComponent(id)}`,
    payload,
  );
  return data;
}

export async function remove(id: string): Promise<void> {
  await http.delete(`/sso/providers/${encodeURIComponent(id)}`);
}

export async function enable(id: string): Promise<SsoProvider> {
  const { data } = await http.post<SsoProvider>(
    `/sso/providers/${encodeURIComponent(id)}/enable`,
  );
  return data;
}

export async function disable(id: string): Promise<SsoProvider> {
  const { data } = await http.post<SsoProvider>(
    `/sso/providers/${encodeURIComponent(id)}/disable`,
  );
  return data;
}

export async function signinLdap(req: {
  provider_id: string;
  username: string;
  password: string;
}): Promise<SigninResponse> {
  const { data } = await http.post<SigninResponse>('/auth/sso/ldap/login', req);
  return data;
}

/**
 * Build the top-level navigation URL for redirect-based OIDC and SAML flows.
 * LDAP uses `signinLdap` instead because it verifies the credentials directly.
 */
export function buildLoginUrl(
  provider: Pick<PublicSsoProvider, 'id' | 'kind'>,
  nextPath?: string,
): string {
  if (provider.kind === 'ldap') {
    throw new Error('LDAP providers use credential login');
  }
  const path =
    provider.kind === 'saml'
      ? '/api/v1/auth/sso/saml/login'
      : '/api/v1/auth/sso/login';
  const params = new URLSearchParams({ provider_id: provider.id });
  if (nextPath) params.set('next', nextPath);
  return `${path}?${params.toString()}`;
}
