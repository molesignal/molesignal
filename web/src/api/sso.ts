import { http } from '@/lib/http';

import type { MfaEnrollRequired, MfaRequired, SigninResponse } from './auth';

export type SsoProviderKind = 'oidc' | 'oauth2' | 'saml' | 'ldap';

/** The preset a provider was made from: its sign-in icon and settings template. */
export type SsoPlatform =
  | 'custom'
  | 'google'
  | 'microsoft'
  | 'github'
  | 'gitlab'
  | 'okta'
  | 'auth0'
  | 'keycloak'
  | 'authentik'
  | 'feishu'
  | 'adfs'
  | 'jumpcloud'
  | 'active_directory'
  | 'openldap'
  | 'freeipa';

type RoleMapping = Record<string, string>;

export interface SsoFieldMapping {
  subject: string;
  email: string;
  displayName: string;
  groups: string;
}

interface AccessConfig {
  groupRoleMapping: RoleMapping;
  defaultRoleId?: string;
  /** When not empty, only members of one of these groups can sign in. */
  allowedGroups: string[];
}

export interface OidcProviderConfig extends AccessConfig {
  issuer: string;
  authorizeUrl: string;
  tokenUrl: string;
  userinfoUrl?: string;
  jwksUri?: string;
  clientId: string;
  clientSecret?: string;
  hasClientSecret?: boolean;
  redirectUri: string;
  scopes: string[];
  fieldMapping: SsoFieldMapping;
}

export interface Oauth2ProviderConfig extends AccessConfig {
  authorizeUrl: string;
  tokenUrl: string;
  userinfoUrl: string;
  clientId: string;
  clientSecret?: string;
  hasClientSecret?: boolean;
  redirectUri: string;
  scopes: string[];
  fieldMapping: SsoFieldMapping;
}

export interface SamlProviderConfig extends AccessConfig {
  spEntityId: string;
  idpEntityId: string;
  idpSsoUrl: string;
  idpX509Cert: string;
  assertionConsumerUrl: string;
  fieldMapping: SsoFieldMapping;
}

export interface LdapProviderConfig extends AccessConfig {
  url: string;
  startTls: boolean;
  bindDn: string;
  bindPassword?: string;
  hasBindPassword?: boolean;
  baseDn: string;
  userFilter: string;
  fieldMapping: SsoFieldMapping;
}

export interface PublicSsoProvider {
  id: string;
  name: string;
  kind: SsoProviderKind;
  platform: SsoPlatform;
}

export interface SsoAssignableRole {
  id: string;
  name: string;
}

/**
 * How the provider's current settings stand with a test sign-in. `stale`: there is a
 * result, and the settings have changed since, so it no longer counts.
 */
export interface SsoTestStanding {
  status: 'untested' | 'passed' | 'failed' | 'stale';
  testId?: string;
  finishedAt?: number;
}

export interface SsoProvider extends PublicSsoProvider {
  orgId: string;
  enabled: boolean;
  oidc?: OidcProviderConfig;
  oauth2?: Oauth2ProviderConfig;
  saml?: SamlProviderConfig;
  ldap?: LdapProviderConfig;
  /** Answered with a provider that was read; a change's answer leaves it out. */
  test?: SsoTestStanding;
  createdAt?: number;
  updatedAt?: number;
}

export interface SsoProviderInput {
  name: string;
  kind: SsoProviderKind;
  platform: SsoPlatform;
  enabled?: boolean;
  oidc?: OidcProviderConfig;
  oauth2?: Oauth2ProviderConfig;
  saml?: SamlProviderConfig;
  ldap?: LdapProviderConfig;
}

/** An issuer's OpenID Connect discovery document, as the settings form uses it. */
export interface OidcDiscovery {
  /** As the issuer publishes it (Auth0's ends with a slash). */
  issuer: string;
  authorizeUrl: string;
  tokenUrl: string;
  userinfoUrl?: string | null;
  jwksUri?: string | null;
  scopesSupported: string[];
}

export interface SamlIdpMetadata {
  idpEntityId: string;
  idpSsoUrl: string;
  idpX509Cert: string;
}

/**
 * The email of an identity that is not bound yet belongs to an existing account: its owner
 * confirms on this page, with a code emailed to the account or with the account password.
 */
export interface SsoLinkRequired {
  /** Ties the confirmation to the browser that signed in. */
  code: string;
  providerName: string;
  organizationName: string;
  /** The account's email with the middle hidden, e.g. `a***@example.com`. */
  emailHint: string;
  /** The instance can email a verification code. */
  emailAvailable: boolean;
  /** The account has a password to confirm with. */
  passwordAllowed: boolean;
}

/**
 * What a single sign-on ends in: a session, a link its owner must confirm first,
 * or (directory sign-ins and confirmed links) the second step of an account with
 * multi-factor authentication.
 */
export interface SsoSignInResult {
  login?: SigninResponse;
  link?: SsoLinkRequired;
  mfa?: MfaRequired;
  enroll?: MfaEnrollRequired;
  /** The local page asked for when the sign-in started. */
  next?: string;
}

/** What the identity provider said about the account that signed in during a test. */
export interface SsoTestedIdentity {
  email: string;
  displayName: string;
  subject: string;
  /** The first ones, when there are very many; `groups_total` counts them all. */
  groups: string[];
  groupsTotal: number;
  emailVerified?: boolean | null;
}

export interface SsoTestedRole {
  id: string;
  name: string;
}

/** What a real sign-in with that identity would do. */
export interface SsoTestedMapping {
  /** `bound`: signs in. `link`: its owner confirms first. `create`: a new account. */
  account: 'bound' | 'link' | 'create';
  /** With the middle hidden for an account outside this organization. */
  accountEmail: string;
  accountName?: string | null;
  /** That account is the person who started the test. */
  isTester: boolean;
  /** That account is already a member of this organization. */
  member: boolean;
  currentRoles: SsoTestedRole[];
  /** The roles after the sign-in. */
  roles: SsoTestedRole[];
  roleSource: 'group_mapping' | 'default_role' | 'signup' | 'unchanged';
  matchedGroups: string[];
}

export interface SsoTestedFailure {
  code: string;
  /** The words of the server or the identity provider, with sensitive values removed. */
  detail?: string | null;
}

/**
 * One test sign-in. It starts no session, creates and links no account and moves no
 * role. `expired`: no answer came back from the identity provider in time.
 */
export interface SsoSigninTest {
  id: string;
  providerId: string;
  providerName: string;
  kind: SsoProviderKind;
  platform: SsoPlatform;
  status: 'pending' | 'passed' | 'failed' | 'expired';
  /** The provider's settings are still the ones that were tested. */
  current: boolean;
  startedBy: string;
  startedByYou: boolean;
  startedAt: number;
  finishedAt?: number;
  identity?: SsoTestedIdentity;
  mapping?: SsoTestedMapping;
  failure?: SsoTestedFailure;
  notes?: string[];
}

export interface SsoSigninTestStart {
  id: string;
  /** Where the browser goes to sign in; it comes back on the result page. */
  authorizationUrl: string;
}

/** An external identity bound to the signed-in account. */
export interface LinkedIdentity {
  id: string;
  providerId: string;
  providerName: string;
  kind: SsoProviderKind;
  platform: SsoPlatform;
  orgId: string;
  orgName: string;
  linkedAt: number;
  lastLoginAt: number;
  /** False for the last identity of an account without a password. */
  canUnlink: boolean;
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

/** Reads `{issuer}/.well-known/openid-configuration` through the server. */
export async function discover(issuer: string): Promise<OidcDiscovery> {
  const { data } = await http.post<OidcDiscovery>('/sso/providers/discover', { issuer });
  return data;
}

/** Reads SAML IdP metadata from its URL or from pasted XML. */
export async function readSamlMetadata(
  source: { url: string } | { xml: string },
): Promise<SamlIdpMetadata> {
  const { data } = await http.post<SamlIdpMetadata>('/sso/providers/saml-metadata', source);
  return data;
}

/** Starts a test sign-in of an OIDC, OAuth 2.0 or SAML provider. */
export async function startSigninTest(providerId: string): Promise<SsoSigninTestStart> {
  const { data } = await http.post<SsoSigninTestStart>(
    `/sso/providers/${encodeURIComponent(providerId)}/tests`,
  );
  return data;
}

/** Checks an account against a directory, without signing in, and answers with the result. */
export async function testDirectorySignin(
  providerId: string,
  credentials: { username: string; password: string },
): Promise<SsoSigninTest> {
  const { data } = await http.post<SsoSigninTest>(
    `/sso/providers/${encodeURIComponent(providerId)}/tests/ldap`,
    credentials,
  );
  return data;
}

export async function getSigninTest(id: string): Promise<SsoSigninTest> {
  const { data } = await http.get<SsoSigninTest>(`/sso/tests/${encodeURIComponent(id)}`);
  return data;
}

export async function signinLdap(req: {
  providerId: string;
  username: string;
  password: string;
  remember: boolean;
}): Promise<SsoSignInResult> {
  const { data } = await http.post<SsoSignInResult>('/auth/sso/ldap/login', req);
  return data;
}

/**
 * Trades the single-use code a redirect sign-in returns with for its outcome.
 * A sign-in sets the session cookies, kept after the browser closes when
 * `remember` is on.
 */
export async function exchange(code: string, remember: boolean): Promise<SsoSignInResult> {
  const { data } = await http.post<SsoSignInResult>('/auth/sso/exchange', { code, remember });
  return data;
}

/** Emails a verification code for a pending link, in the page's language. */
export async function sendLinkCode(code: string, locale: string): Promise<void> {
  await http.post('/auth/sso/link/email', { code, locale });
}

/** Confirms a pending link with the emailed code or the account password, and signs in. */
export async function confirmLink(
  code: string,
  proof: { emailCode: string } | { password: string },
  remember: boolean,
): Promise<SsoSignInResult> {
  const { data } = await http.post<SsoSignInResult>('/auth/sso/link/confirm', {
    code,
    ...('emailCode' in proof ? { email_code: proof.emailCode } : { password: proof.password }),
    remember,
  });
  return data;
}

export async function listLinkedIdentities(): Promise<LinkedIdentity[]> {
  const { data } = await http.get<LinkedIdentity[]>('/me/sso-identities');
  return data;
}

export async function unlinkIdentity(id: string): Promise<void> {
  await http.delete(`/me/sso-identities/${encodeURIComponent(id)}`);
}

/**
 * Build the top-level navigation URL for redirect-based OIDC, OAuth 2.0 and SAML
 * flows. LDAP uses `signinLdap` instead because it verifies the credentials directly.
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
  const params = new URLSearchParams({ providerId: provider.id });
  if (nextPath) params.set('next', nextPath);
  return `${path}?${params.toString()}`;
}
