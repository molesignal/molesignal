import type { FactorProof, SigninResult } from '@/api/auth';
import { http } from '@/lib/http';
import type {
  PasskeyCeremony,
  PasskeyCreationOptions,
  PasskeyRegistration,
  PasskeyRequestOptions,
} from '@/lib/webauthn';

export type { FactorProof } from '@/api/auth';

/** One of the account's passkeys. Without a name, the page says where it is kept. */
export interface Passkey {
  id: string;
  name: string;
  /** The authenticator's model, in hex; empty when the browser hides it. */
  aaguid: string | null;
  createdAt: number;
  lastUsedAt: number | null;
}

/** Why passkeys don't work on this instance. */
export type PasskeysUnavailable = 'not_configured' | 'ip_address' | 'not_https';

/**
 * What adding a second factor asks for first: an existing second factor, the
 * account's password, nothing (no password, and signed in a moment ago), or a
 * fresh sign-in (no password, and signed in a while ago).
 */
export type AddingRequires = 'factor' | 'password' | 'nothing' | 'sign_in_again';

/** The account's own multi-factor authentication. */
export interface MfaStatus {
  totpEnabled: boolean;
  totpEnabledAt: number | null;
  recoveryCodesLeft: number;
  passkeys: Passkey[];
  passkeysUnavailable: PasskeysUnavailable | null;
  addingRequires: AddingRequires;
  /** The account's organizations that ask for it: the last second factor has to stay. */
  requiredBy: string[];
}

/** What confirms a change: a second factor, or the password before there is one. */
export type Proof = FactorProof | { method: 'password'; password: string };

/** A new authenticator waiting for its first code. */
export interface TotpSetup {
  /** Sent back with the first code. */
  setup: string;
  /** The key for typing in by hand, in groups of four. */
  secret: string;
  otpauthUrl: string;
  /** The same address as a QR code (SVG markup from the server). */
  qrSvg: string;
  expiresAt: number;
}

export async function status(): Promise<MfaStatus> {
  const { data } = await http.get<MfaStatus>('/me/mfa');
  return data;
}

/** Starts setting up an authenticator, after confirming it's the account's owner. */
export async function setupTotp(proof?: Proof): Promise<TotpSetup> {
  const { data } = await http.post<TotpSetup>('/me/mfa/totp/setup', proof ? { proof } : {});
  return data;
}

/**
 * Turns the authenticator on with its first code. When that turns multi-factor
 * authentication on, the answer holds the recovery codes, shown this once, and
 * other devices are signed out; so they are when it replaces an authenticator.
 */
export async function confirmTotp(
  setup: string,
  code: string,
): Promise<{ recoveryCodes: string[] | null }> {
  const { data } = await http.post<{ recoveryCodes: string[] | null }>('/me/mfa/totp/confirm', {
    setup,
    code,
  });
  return data;
}

/** Removes the authenticator. Other devices are signed out. */
export async function disableTotp(proof: Proof): Promise<void> {
  await http.post('/me/mfa/totp/disable', proof);
}

/** A new set of recovery codes; the old ones stop working. */
export async function regenerateRecoveryCodes(proof: Proof): Promise<string[]> {
  const { data } = await http.post<{ recoveryCodes: string[] }>('/me/mfa/recovery-codes', proof);
  return data.recoveryCodes;
}

/** A passkey check that confirms a change on the account's settings. */
export async function passkeyProof(): Promise<PasskeyCeremony<PasskeyRequestOptions>> {
  const { data } = await http.post<PasskeyCeremony<PasskeyRequestOptions>>(
    '/me/mfa/passkey-proof',
  );
  return data;
}

/** The options for creating a passkey, after confirming it's the account's owner. */
export async function passkeyOptions(
  proof?: Proof,
): Promise<PasskeyCeremony<PasskeyCreationOptions>> {
  const { data } = await http.post<PasskeyCeremony<PasskeyCreationOptions>>(
    '/me/passkeys/options',
    proof ? { proof } : {},
  );
  return data;
}

/**
 * Keeps the passkey the browser created. When it turns multi-factor authentication
 * on, the answer holds the recovery codes, shown this once, and other devices are
 * signed out.
 */
export async function addPasskey(req: {
  ceremony: string;
  credential: PasskeyRegistration;
  name: string;
}): Promise<{ passkey: Passkey; recoveryCodes: string[] | null }> {
  const { data } = await http.post<{ passkey: Passkey; recoveryCodes: string[] | null }>(
    '/me/passkeys',
    req,
  );
  return data;
}

export async function renamePasskey(id: string, name: string): Promise<void> {
  await http.patch(`/me/passkeys/${encodeURIComponent(id)}`, { name });
}

/** Removes a passkey. Other devices are signed out. */
export async function removePasskey(id: string, proof: Proof): Promise<void> {
  await http.post(`/me/passkeys/${encodeURIComponent(id)}/remove`, proof);
}

/** Starts setting up an authenticator during a sign-in that asks for a second factor. */
export async function enrollTotp(challenge: string): Promise<TotpSetup> {
  const { data } = await http.post<TotpSetup>('/auth/signin/enroll/totp', { challenge });
  return data;
}

/** Turns it on with its first code and finishes the sign-in. */
export async function confirmEnrolledTotp(req: {
  challenge: string;
  setup: string;
  code: string;
}): Promise<SigninResult> {
  const { data } = await http.post<SigninResult>('/auth/signin/enroll/totp/confirm', req);
  return data;
}

/** The options for creating a passkey during a sign-in that asks for a second factor. */
export async function enrollPasskeyOptions(
  challenge: string,
): Promise<PasskeyCeremony<PasskeyCreationOptions>> {
  const { data } = await http.post<PasskeyCeremony<PasskeyCreationOptions>>(
    '/auth/signin/enroll/passkey/options',
    { challenge },
  );
  return data;
}

/** Keeps the passkey the browser created and finishes the sign-in. */
export async function enrollPasskey(req: {
  challenge: string;
  ceremony: string;
  credential: PasskeyRegistration;
  name: string;
}): Promise<SigninResult> {
  const { data } = await http.post<SigninResult>('/auth/signin/enroll/passkey', req);
  return data;
}

/** What entering the current organization asks of a sign-in. */
export interface SigninPolicy {
  requireMfa: boolean;
  ssoOnly: boolean;
}

export async function signinPolicy(): Promise<SigninPolicy> {
  const { data } = await http.get<SigninPolicy>('/settings/signin-policy');
  return data;
}

export async function updateSigninPolicy(policy: SigninPolicy): Promise<SigninPolicy> {
  const { data } = await http.put<SigninPolicy>('/settings/signin-policy', policy);
  return data;
}

/** Signs out every other device; this browser stays signed in. */
export async function revokeOtherSessions(): Promise<void> {
  await http.post('/me/sessions/revoke-others');
}

/** An administrator turns a member's multi-factor authentication off. */
export async function resetMember(userId: string): Promise<void> {
  await http.post(`/users/${encodeURIComponent(userId)}/mfa/reset`);
}
