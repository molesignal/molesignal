/**
 * How this browser last signed in, so the sign-in page can point at that way in and fill the
 * account in on the next visit. Only the way and the account's name are kept, never a password
 * or a token. Storage can be unavailable (private mode), and the page then simply has nothing
 * to point at.
 */

export type SigninChannel =
  | { kind: 'password' }
  | { kind: 'passkey' }
  /** A single sign-on provider or an LDAP directory. */
  | { kind: 'provider'; id: string };

export interface LastSignin {
  channel: SigninChannel;
  /** The email or directory username; a passkey or a provider may not say. */
  account?: string;
}

const LAST_SIGNIN_KEY = 'molesignal.signin.last';

function channelFrom(value: unknown): SigninChannel | null {
  if (typeof value !== 'object' || value === null) return null;

  const { kind, id } = value as { kind?: unknown; id?: unknown };

  if (kind === 'password' || kind === 'passkey') return { kind };
  if (kind === 'provider' && typeof id === 'string' && id) return { kind, id };

  return null;
}

/** What the last successful sign-in here kept, or `null` when nothing usable is there. */
export function readLastSignin(): LastSignin | null {
  try {
    const stored: unknown = JSON.parse(window.localStorage.getItem(LAST_SIGNIN_KEY) ?? 'null');

    if (typeof stored !== 'object' || stored === null) return null;

    const { channel, account } = stored as { channel?: unknown; account?: unknown };
    const known = channelFrom(channel);

    if (!known) return null;

    return typeof account === 'string' && account ? { channel: known, account } : { channel: known };
  } catch {
    return null;
  }
}

/** Keeps a sign-in that succeeded; `account` is who it turned out to be when `last` doesn't say. */
export function keepLastSignin(last: LastSignin, account?: string): void {
  const named = last.account ?? account;

  try {
    window.localStorage.setItem(
      LAST_SIGNIN_KEY,
      JSON.stringify(named ? { channel: last.channel, account: named } : { channel: last.channel }),
    );
  } catch {
    // Without storage the next visit just has nothing to point at.
  }
}

/**
 * What has to survive the trip to the identity provider and back: "remember me" and the provider
 * chosen. It waits in this tab's session storage until the returned code is exchanged.
 */
const SSO_REMEMBER_KEY = 'molesignal.sso.remember';
const SSO_PROVIDER_KEY = 'molesignal.sso.provider';

export function keepSsoTrip(remember: boolean, providerId: string): void {
  try {
    window.sessionStorage.setItem(SSO_REMEMBER_KEY, remember ? '1' : '0');
    window.sessionStorage.setItem(SSO_PROVIDER_KEY, providerId);
  } catch {
    // Storage can be unavailable (private mode); the session is then not remembered.
  }
}

export function takeSsoTrip(): { remember: boolean; providerId: string | null } {
  try {
    const remember = window.sessionStorage.getItem(SSO_REMEMBER_KEY) === '1';
    const providerId = window.sessionStorage.getItem(SSO_PROVIDER_KEY);
    window.sessionStorage.removeItem(SSO_REMEMBER_KEY);
    window.sessionStorage.removeItem(SSO_PROVIDER_KEY);

    return { remember, providerId };
  } catch {
    return { remember: false, providerId: null };
  }
}
