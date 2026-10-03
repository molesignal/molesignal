import type { AuthContext } from './index';

/**
 * Hands a signed-in session to tabs that are opened later.
 *
 * Without "remember me" the session lives in sessionStorage, which belongs to
 * one tab: a tab opened by hand starts empty and would send the user to the
 * sign-in page even though another tab is signed in. Persisting the session
 * would cure that but also keep it across a browser restart, so a new tab asks
 * the tabs that are already open over a BroadcastChannel instead and keeps the
 * answer in its own sessionStorage. Close every tab and the session is gone.
 *
 * Any script on this origin can ask the question, so an open session is as
 * reachable as a "remember me" session in localStorage would be.
 */

const CHANNEL_NAME = 'molesignal-auth-session';

export interface SharedSession {
  token: string;
  ctx: AuthContext;
}

type Message =
  | { type: 'request'; id: string }
  | { type: 'offer'; id: string; session: SharedSession; activeAt: number }
  | { type: 'sign-out'; from: string };

/** The part of BroadcastChannel this module uses. */
export interface ChannelLike {
  postMessage: (message: unknown) => void;
  addEventListener: (type: 'message', listener: (event: MessageEvent) => void) => void;
  removeEventListener: (type: 'message', listener: (event: MessageEvent) => void) => void;
}

export interface TabSessionOptions {
  /** Opens the shared channel; `null` where BroadcastChannel does not exist. */
  openChannel?: () => ChannelLike | null;
  now?: () => number;
  newId?: () => string;
  /** How long a new tab waits for any open tab to answer. */
  timeoutMs?: number;
  /** After the first answer, how long to keep listening for a fresher one. */
  settleMs?: number;
}

export interface SessionLender {
  getSession: () => SharedSession | null;
  /** Another tab signed out on purpose. */
  onSignOut: () => void;
}

// Pointer or keyboard input, or the tab coming to the front, marks it as the
// one the user was just in.
const ACTIVITY_EVENTS = ['focus', 'pointerdown', 'keydown'] as const;

function openDefaultChannel(): ChannelLike | null {
  return typeof globalThis.BroadcastChannel === 'undefined'
    ? null
    : new globalThis.BroadcastChannel(CHANNEL_NAME);
}

// randomUUID only exists in secure contexts and the app is also served over
// plain HTTP.
function randomId(): string {
  return (
    globalThis.crypto?.randomUUID?.() ??
    `${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`
  );
}

function isSession(value: unknown): value is SharedSession {
  if (typeof value !== 'object' || value === null) return false;
  const { token, ctx } = value as Record<string, unknown>;
  if (typeof token !== 'string' || token === '') return false;
  if (typeof ctx !== 'object' || ctx === null) return false;
  const context = ctx as Record<string, unknown>;
  return (
    typeof context.user_id === 'string' &&
    typeof context.org_id === 'string' &&
    typeof context.display_role === 'string' &&
    Array.isArray(context.roles)
  );
}

function parseMessage(data: unknown): Message | null {
  if (typeof data !== 'object' || data === null) return null;
  const message = data as Record<string, unknown>;
  switch (message.type) {
    case 'request':
      return typeof message.id === 'string' ? { type: 'request', id: message.id } : null;
    case 'offer':
      return typeof message.id === 'string' &&
        typeof message.activeAt === 'number' &&
        isSession(message.session)
        ? {
            type: 'offer',
            id: message.id,
            session: message.session,
            activeAt: message.activeAt,
          }
        : null;
    case 'sign-out':
      return typeof message.from === 'string'
        ? { type: 'sign-out', from: message.from }
        : null;
    default:
      return null;
  }
}

export function createTabSessionSharing(options: TabSessionOptions = {}) {
  const openChannel = options.openChannel ?? openDefaultChannel;
  const now = options.now ?? Date.now;
  const newId = options.newId ?? randomId;
  const timeoutMs = options.timeoutMs ?? 300;
  const settleMs = options.settleMs ?? 40;
  const tabId = newId();

  // One channel per page, opened on first use so that merely importing the
  // auth store never holds a channel open.
  let channel: ChannelLike | null | undefined;
  const getChannel = (): ChannelLike | null => {
    if (channel === undefined) channel = openChannel();
    return channel;
  };

  /**
   * Asks the open tabs for their session. Resolves with the one from the tab
   * the user was most recently in, or `null` when nobody answers in time.
   */
  function borrowSession(): Promise<SharedSession | null> {
    const shared = getChannel();
    if (!shared) return Promise.resolve(null);
    const requestId = newId();

    return new Promise((resolve) => {
      let best: { session: SharedSession; activeAt: number } | null = null;
      let settleTimer: ReturnType<typeof setTimeout> | undefined;

      const finish = () => {
        clearTimeout(giveUpTimer);
        clearTimeout(settleTimer);
        shared.removeEventListener('message', onMessage);
        resolve(best?.session ?? null);
      };
      const onMessage = (event: MessageEvent) => {
        const message = parseMessage(event.data);
        if (message?.type !== 'offer' || message.id !== requestId) return;
        if (!best || message.activeAt > best.activeAt) {
          best = { session: message.session, activeAt: message.activeAt };
        }
        settleTimer ??= setTimeout(finish, settleMs);
      };

      const giveUpTimer = setTimeout(finish, timeoutMs);
      shared.addEventListener('message', onMessage);
      shared.postMessage({ type: 'request', id: requestId } satisfies Message);
    });
  }

  /**
   * Answers other tabs' requests with this tab's session for as long as it is
   * signed in, and reports when another tab signs out. Returns the stop function.
   */
  function lendSession({ getSession, onSignOut }: SessionLender): () => void {
    const shared = getChannel();
    if (!shared) return () => {};

    let activeAt = now();
    const touch = () => {
      activeAt = now();
    };
    const onVisibility = () => {
      if (document.visibilityState === 'visible') touch();
    };
    const onMessage = (event: MessageEvent) => {
      const message = parseMessage(event.data);
      if (message?.type === 'request') {
        const session = getSession();
        if (session) {
          shared.postMessage({
            type: 'offer',
            id: message.id,
            session,
            activeAt,
          } satisfies Message);
        }
      } else if (message?.type === 'sign-out' && message.from !== tabId) {
        onSignOut();
      }
    };

    shared.addEventListener('message', onMessage);
    for (const type of ACTIVITY_EVENTS) window.addEventListener(type, touch, true);
    document.addEventListener('visibilitychange', onVisibility);
    return () => {
      shared.removeEventListener('message', onMessage);
      for (const type of ACTIVITY_EVENTS) window.removeEventListener(type, touch, true);
      document.removeEventListener('visibilitychange', onVisibility);
    };
  }

  /** The user signed out here on purpose: every other tab should follow. */
  function announceSignOut(): void {
    getChannel()?.postMessage({ type: 'sign-out', from: tabId } satisfies Message);
  }

  return { borrowSession, lendSession, announceSignOut };
}

export const tabSessionSharing = createTabSessionSharing();
