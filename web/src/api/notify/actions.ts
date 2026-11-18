import axios from 'axios';

/** What a one-time link from a notification does to the alert it came with. */
export type NotifyActionKind = 'acknowledge' | 'resolve';

/** Why a link did not act: it expired (or never was one), it was used, the alert no longer needs
 * it, or the person it was sent to may no longer act on the alert. */
export type NotifyActionRefusal = 'expired' | 'used' | 'inactive' | 'no_access';

/** A one-time link as its confirm page shows it; reading it does nothing to the alert. */
export interface NotifyActionLink {
  action: NotifyActionKind;
  alert: {
    id: string;
    title: string;
    severity: string;
    status: string;
    /** Microseconds. */
    startedAt: number;
  };
  /** Microseconds. */
  expiresAt: number;
  /** Microseconds; null while the link has not been used. */
  usedAt: number | null;
  /** The language of the notification the link was sent in, such as `zh-cn`. */
  language: string;
}

export interface NotifyActionResult {
  status: 'acknowledged' | 'resolved';
}

// The token is the credential: no session is needed, and none is renewed or asked for.
const publicHttp = axios.create({
  baseURL: '/api/v1/public/notify/actions',
  timeout: 30_000,
});

/** What a link would do, without doing it: mail scanners open links before people do. */
export async function peekAction(token: string): Promise<NotifyActionLink> {
  const { data } = await publicHttp.get<NotifyActionLink>(`/${encodeURIComponent(token)}`);
  return data;
}

/** Acknowledges or resolves the alert as the person the link was sent to. */
export async function takeAction(token: string): Promise<NotifyActionResult> {
  const { data } = await publicHttp.post<NotifyActionResult>(`/${encodeURIComponent(token)}`);
  return data;
}
