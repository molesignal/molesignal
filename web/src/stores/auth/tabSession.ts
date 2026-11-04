/**
 * Tells the other open tabs when this browser's session changes.
 *
 * The session lives in the browser's cookies, which every tab shares: when one
 * tab signs out, switches organization or signs in as someone else, the others
 * still show the old session. The tab announces the change over a
 * BroadcastChannel and the others follow at once. A tab that misses the
 * message learns it from the server, which refuses a request made for another
 * session than the cookie's (see `lib/http.ts`).
 */

const CHANNEL_NAME = 'molesignal-auth-session';

type Message =
  | { type: 'sign-out'; from: string }
  | { type: 'session'; from: string; session: string };

/** The part of BroadcastChannel this module uses. */
export interface ChannelLike {
  postMessage: (message: unknown) => void;
  addEventListener: (type: 'message', listener: (event: MessageEvent) => void) => void;
  removeEventListener: (type: 'message', listener: (event: MessageEvent) => void) => void;
}

export interface TabSessionOptions {
  /** Opens the shared channel; `null` where BroadcastChannel does not exist. */
  openChannel?: () => ChannelLike | null;
  newId?: () => string;
}

export interface SessionListener {
  /** Another tab signed out on purpose. */
  onSignOut: () => void;
  /** Another tab now shows `session` (`<userId>:<orgId>`). */
  onSession: (session: string) => void;
}

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

function parseMessage(data: unknown): Message | null {
  if (typeof data !== 'object' || data === null) return null;
  const message = data as Record<string, unknown>;
  if (typeof message.from !== 'string') return null;
  if (message.type === 'sign-out') return { type: 'sign-out', from: message.from };
  if (message.type === 'session' && typeof message.session === 'string') {
    return { type: 'session', from: message.from, session: message.session };
  }
  return null;
}

export function createTabSessionSharing(options: TabSessionOptions = {}) {
  const openChannel = options.openChannel ?? openDefaultChannel;
  const tabId = (options.newId ?? randomId)();

  // One channel per page, opened on first use so that merely importing the
  // auth store never holds a channel open.
  let channel: ChannelLike | null | undefined;
  const getChannel = (): ChannelLike | null => {
    if (channel === undefined) channel = openChannel();
    return channel;
  };

  /** Hears what the other tabs announce. Returns the stop function. */
  function listen({ onSignOut, onSession }: SessionListener): () => void {
    const shared = getChannel();
    if (!shared) return () => {};

    const onMessage = (event: MessageEvent) => {
      const message = parseMessage(event.data);
      if (!message || message.from === tabId) return;
      if (message.type === 'sign-out') onSignOut();
      else onSession(message.session);
    };

    shared.addEventListener('message', onMessage);
    return () => shared.removeEventListener('message', onMessage);
  }

  /** The user signed out here on purpose: every other tab should follow. */
  function announceSignOut(): void {
    getChannel()?.postMessage({ type: 'sign-out', from: tabId } satisfies Message);
  }

  /** This tab now shows `session`; the cookie the others send belongs to it too. */
  function announceSession(session: string): void {
    getChannel()?.postMessage({ type: 'session', from: tabId, session } satisfies Message);
  }

  return { listen, announceSignOut, announceSession };
}

export const tabSessionSharing = createTabSessionSharing();
