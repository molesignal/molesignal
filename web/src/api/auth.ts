import { http } from '@/lib/http';
import type { PasskeyAssertion, PasskeyCeremony, PasskeyRequestOptions } from '@/lib/webauthn';
import type { AssignedRole } from '@/stores/auth';

/**
 * Who signed in. The session itself comes back as HttpOnly cookies the page
 * cannot read.
 */
export interface SigninResponse {
  userId: string;
  email?: string;
  displayName?: string;
  orgId: string;
  orgName?: string;
  displayRole: string;
  roles: AssignedRole[];
  scope?: string;
}

/** A second factor that can finish a sign-in. */
export type MfaMethod = 'totp' | 'recovery_code' | 'passkey';

/** A second factor handed in: a typed code, or a signature from a passkey. */
export type FactorProof =
  | { method: 'totp' | 'recovery_code'; code: string }
  | { method: 'passkey'; ceremony: string; credential: PasskeyAssertion };

/** The second step a sign-in waits for when the account has multi-factor authentication. */
export interface MfaRequired {
  /** Ties the second step to this browser. */
  challenge: string;
  methods: MfaMethod[];
  expiresAt: number;
}

/**
 * The organization being signed in to asks for multi-factor authentication and the
 * account has no second factor yet: it is set up on the spot, and the session comes
 * with it.
 */
export interface MfaEnrollRequired {
  /** Ties the setup to this browser. */
  challenge: string;
  methods: ('passkey' | 'totp')[];
  organizationName: string;
  expiresAt: number;
}

/**
 * What the first step of a sign-in ends in: the session (its cookies are set),
 * the second step the account asks for, or a second factor to set up first.
 */
export interface SigninResult {
  login?: SigninResponse;
  mfa?: MfaRequired;
  enroll?: MfaEnrollRequired;
  /** How many recovery codes are left, after signing in with one. */
  recoveryCodesLeft?: number | null;
  /** After setting up a second factor on the spot: shown this once. */
  recoveryCodes?: string[] | null;
}

export async function signin(req: {
  email: string;
  password: string;
  /** Keep the session after the browser closes. */
  remember: boolean;
}): Promise<SigninResult> {
  const { data } = await http.post<SigninResult>('/auth/signin', req);
  return data;
}

/** Finishes a sign-in with a code from the authenticator app, a recovery code or a passkey. */
export async function verifySignin(
  req: { challenge: string } & FactorProof,
): Promise<SigninResult> {
  const { data } = await http.post<SigninResult>('/auth/signin/verify', req);
  return data;
}

/** A passkey check for the second step of the sign-in waiting on `challenge`. */
export async function secondStepPasskey(
  challenge: string,
): Promise<PasskeyCeremony<PasskeyRequestOptions>> {
  const { data } = await http.post<PasskeyCeremony<PasskeyRequestOptions>>(
    '/auth/signin/verify/passkey',
    { challenge },
  );
  return data;
}

/** A passkey sign-in without a password: the passkey says whose account it is. */
export async function passkeySigninOptions(): Promise<PasskeyCeremony<PasskeyRequestOptions>> {
  const { data } = await http.post<PasskeyCeremony<PasskeyRequestOptions>>(
    '/auth/signin/passkey/options',
  );
  return data;
}

export async function signinWithPasskey(req: {
  ceremony: string;
  credential: PasskeyAssertion;
  /** Keep the session after the browser closes. */
  remember: boolean;
}): Promise<SigninResult> {
  const { data } = await http.post<SigninResult>('/auth/signin/passkey', req);
  return data;
}

/** Ends the session: the server drops the browser's session cookies. */
export async function signout(): Promise<void> {
  await http.post('/auth/signout');
}

/** The answer to asking to sign up: the same whether or not the email has an account. */
export interface SignupAccepted {
  /** "verify_email" (a link is on its way) or "pending" (waits for an administrator). */
  status: 'verify_email' | 'pending';
}

/** Ask to sign up on an instance that sends email: the name and password come after the link. */
export async function requestSignupLink(req: {
  email: string;
  locale: string;
}): Promise<SignupAccepted> {
  const { data } = await http.post<SignupAccepted>('/auth/signup', req);
  return data;
}

/** Sign up on an instance that cannot send email: an administrator has to approve it. */
export async function applyForSignup(req: {
  email: string;
  displayName: string;
  password: string;
}): Promise<SignupAccepted> {
  const { data } = await http.post<SignupAccepted>('/auth/signup', req);
  return data;
}

/** The address a signup link proves. Fails when the link is no longer good. */
export async function verifySignupLink(token: string): Promise<{ email: string }> {
  const { data } = await http.post<{ email: string }>('/auth/signup/verify', { token });
  return data;
}

export interface SignupResponse {
  /** "active"（已激活，已经登录）、"pending"（待审批，没有登录）或 "signin_required"（去登录时现场开启多因素认证）。 */
  status: 'active' | 'pending' | 'signin_required';
  userId: string;
  email: string;
  displayName: string;
  orgId: string | null;
  orgName: string | null;
  displayRole: string | null;
  roles: AssignedRole[];
}

/** Set the name and password from a signup link; the account exists from here on. */
export async function completeSignup(req: {
  token: string;
  displayName: string;
  password: string;
}): Promise<SignupResponse> {
  const { data } = await http.post<SignupResponse>('/auth/signup/complete', req);
  return data;
}

export async function forgotPassword(req: {
  email: string;
  locale?: string;
}): Promise<{ accepted: boolean }> {
  const { data } = await http.post<{ accepted: boolean }>('/auth/forgot-password', req);
  return data;
}

export async function resetPassword(req: {
  token: string;
  password: string;
}): Promise<{ reset: boolean }> {
  const { data } = await http.post<{ reset: boolean }>('/auth/reset-password', req);
  return data;
}
