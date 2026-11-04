import type * as ssoApi from '@/api/sso';
import type { StatusTone } from '@/shell/StatusWord';

/**
 * The classes of failure the server reports, each with a title and advice in
 * `sso_providers.test.failures`. A class this page doesn't know reads as `unknown`.
 */
export const TEST_FAILURE_CODES = [
  'idp_error',
  'no_code',
  'network',
  'token_exchange',
  'no_id_token',
  'id_token_issuer',
  'id_token_audience',
  'id_token_expired',
  'id_token_nonce',
  'id_token_invalid',
  'jwks',
  'userinfo',
  'claim_missing',
  'saml_status',
  'saml_no_response',
  'saml_malformed',
  'saml_certificate',
  'saml_signature',
  'saml_time',
  'saml_audience',
  'saml_recipient',
  'ldap_connect',
  'ldap_service_bind',
  'ldap_search',
  'ldap_no_entry',
  'ldap_many_entries',
  'ldap_missing_attribute',
  'ldap_rejected',
  'group_not_allowed',
  'email_unverified',
  'provider_unrestricted',
  'invalid_email',
  'email_domain',
  'org_disabled',
  'account_unavailable',
  'account_confined',
  'role_missing',
  'directory_blocked',
  'settings_changed',
  'unknown',
] as const;

export type TestFailureCode = (typeof TEST_FAILURE_CODES)[number];

/** Things worth a look even when a test passes, each said in `sso_providers.test.notes`. */
export const TEST_NOTE_CODES = [
  'no_groups',
  'groups_unmapped',
  'another_account',
  'roles_change',
  'second_factor',
] as const;

export type TestNoteCode = (typeof TEST_NOTE_CODES)[number];

export function failureCodeOf(code: string | undefined): TestFailureCode {
  return TEST_FAILURE_CODES.find((known) => known === code) ?? 'unknown';
}

export function knownNotes(notes: readonly string[] | undefined): TestNoteCode[] {
  return TEST_NOTE_CODES.filter((note) => notes?.includes(note));
}

export const STANDING_TONE: Record<ssoApi.SsoTestStanding['status'], StatusTone> = {
  untested: 'dim',
  passed: 'green',
  failed: 'red',
  stale: 'orange',
};

const NOT_TESTED: ssoApi.SsoTestStanding = { status: 'untested' };

export function standingOf(provider: Pick<ssoApi.SsoProvider, 'test'>): ssoApi.SsoTestStanding {
  return provider.test ?? NOT_TESTED;
}

export function testResultPath(id: string): string {
  return `/iam/sso/tests/${encodeURIComponent(id)}`;
}

/** The providers "only allow single sign-on" would rely on. LDAP only checks a password. */
export function singleSignOnProviders(providers: readonly ssoApi.SsoProvider[]): ssoApi.SsoProvider[] {
  return providers.filter((provider) => provider.enabled && provider.kind !== 'ldap');
}

export interface EnforcementCheck {
  /** Have passed a test with their current settings. */
  proven: ssoApi.SsoProvider[];
  unproven: ssoApi.SsoProvider[];
}

/** What is known about the providers before they become the only way in. */
export function enforcementCheck(providers: readonly ssoApi.SsoProvider[]): EnforcementCheck {
  const passed = (provider: ssoApi.SsoProvider) => standingOf(provider).status === 'passed';
  return {
    proven: providers.filter(passed),
    unproven: providers.filter((provider) => !passed(provider)),
  };
}

/** A member's roles change at sign-in when the two lists hold different roles. */
export function rolesChange(mapping: ssoApi.SsoTestedMapping): boolean {
  const ids = (roles: readonly ssoApi.SsoTestedRole[]) => roles.map((role) => role.id).sort().join('\n');
  return mapping.member && ids(mapping.currentRoles) !== ids(mapping.roles);
}
