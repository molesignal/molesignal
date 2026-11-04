import * as React from 'react';

import type { PublicSsoProvider } from '@/api/sso';

import type { LastSignin } from './lastSignin';

/**
 * Where the sign-in view is: the ways in, then one question at a time. `directory` is the LDAP
 * provider when the account is a directory's, and `null` for the instance's own email and password.
 */
export type SigninStep =
  | { name: 'channels' }
  | { name: 'identifier'; directory: PublicSsoProvider | null }
  | { name: 'password'; directory: PublicSsoProvider | null };

/** What the page shows; `waiting` while it can't tell yet whether there is any way in to list. */
export type VisibleStep = SigninStep | { name: 'waiting' };

/** One way in on the list. Only the way used last time says so, and whose account it was. */
export type ChannelRow =
  /** The account that signed in with its password last time: straight on to the password. */
  | { kind: 'account'; account: string; lastUsed: true }
  | { kind: 'passkey'; lastUsed: boolean; account?: string }
  | { kind: 'provider'; provider: PublicSsoProvider; lastUsed: boolean; account?: string };

export function channelRows(
  providers: PublicSsoProvider[],
  passkey: boolean,
  last: LastSignin | null,
): ChannelRow[] {
  const channel = last?.channel;
  const account = last?.account;
  const named = account ? { account } : {};
  const rows: ChannelRow[] = [];

  if (channel?.kind === 'password' && account) {
    rows.push({ kind: 'account', account, lastUsed: true });
  }

  if (passkey) {
    const lastUsed = channel?.kind === 'passkey';
    rows.push({ kind: 'passkey', lastUsed, ...(lastUsed ? named : {}) });
  }

  for (const provider of providers) {
    const lastUsed = channel?.kind === 'provider' && channel.id === provider.id;
    rows.push({ kind: 'provider', provider, lastUsed, ...(lastUsed ? named : {}) });
  }

  return rows;
}

/** The email to fill in: a directory's username isn't one, and has no place in the email field. */
export function rememberedEmail(last: LastSignin | null): string {
  return last?.account?.includes('@') ? last.account : '';
}

/**
 * How long the page waits before it decides there is nothing to list: a slow answer about the
 * providers must not hold the email and password back.
 */
const CHANNELS_WAIT_MS = 1_000;

export function useChannelsKnown(loading: boolean): boolean {
  const [waited, setWaited] = React.useState(false);

  React.useEffect(() => {
    const timer = window.setTimeout(() => setWaited(true), CHANNELS_WAIT_MS);

    return () => window.clearTimeout(timer);
  }, []);

  return !loading || waited;
}

function visibleStep(step: SigninStep, rows: ChannelRow[], known: boolean): VisibleStep {
  if (step.name !== 'channels' || rows.length > 0) return step;

  // Nothing to choose from: the sign-in starts with the email.
  return known ? { name: 'identifier', directory: null } : { name: 'waiting' };
}

/** The steps taken so far, so that "back" goes one question back. */
export function useSigninSteps(rows: ChannelRow[], known: boolean) {
  const [trail, setTrail] = React.useState<SigninStep[]>([{ name: 'channels' }]);
  const current: SigninStep = trail.at(-1) ?? { name: 'channels' };
  const previous = trail.at(-2);
  const step = visibleStep(current, rows, known);

  return {
    step,
    /** Going back would change what's on screen; with nothing to list, the email is the start. */
    canGoBack: previous !== undefined && visibleStep(previous, rows, known).name !== step.name,
    back: () => setTrail((steps) => (steps.length > 1 ? steps.slice(0, -1) : steps)),
    /** Back to the ways in, for a workspace that turned out to let only its providers in. */
    restart: () => setTrail([{ name: 'channels' }]),
    /** On to the question for the account. */
    askIdentifier: (directory: PublicSsoProvider | null) =>
      setTrail((steps) => [...steps, { name: 'identifier', directory }]),
    /** On to the password. */
    askPassword: (directory: PublicSsoProvider | null) =>
      setTrail((steps) => [...steps, { name: 'password', directory }]),
    /** The account is already known; "back" still leads to the question, to change it. */
    skipToPassword: (directory: PublicSsoProvider | null) =>
      setTrail((steps) => [
        ...steps,
        { name: 'identifier', directory },
        { name: 'password', directory },
      ]),
    /**
     * Someone started typing an email that was shown only because nothing was listed yet: a
     * late answer about the providers no longer takes it away.
     */
    stayOnEmail: () => {
      if (current.name === 'channels' && step.name === 'identifier') {
        setTrail((steps) => [...steps, { name: 'identifier', directory: null }]);
      }
    },
  };
}
