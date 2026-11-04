import type { Incident, Severity } from '@/types/alerting';

/** Which tab of the bell a notification sits under, besides All. */
export type NotificationCategory = 'alerts' | 'approvals';

export interface AppNotification {
  /** Stable for as long as it is the same thing to read: what is read is remembered by this. */
  id: string;
  category: NotificationCategory;
  /** Already in the reader's language. */
  title: string;
  description?: string | undefined;
  /** Where pressing it leads. */
  to?: string | undefined;
  /** Named in its colour before the description. */
  severity?: Severity | undefined;
  /** Set on an incident that can be acknowledged from the list. */
  incidentId?: string | undefined;
}

/** The bell and the palette sit on every page: an answer they cannot read costs them a source, not the page. */
export function rowsOf<T>(data: readonly T[] | undefined): readonly T[] {
  return Array.isArray(data) ? data : [];
}

/**
 * An incident the bell should ring for: nobody has answered it, it is not
 * snoozed, and it is this person's to answer, or nobody's yet.
 */
export function isWaitingOn(incident: Incident, userId: string): boolean {
  return (
    incident.status === 'open' &&
    !incident.snoozedUntil &&
    (incident.assignees.length === 0 || incident.assignees.includes(userId))
  );
}

const SEVERITY_RANK: Record<Severity, number> = { critical: 0, error: 1, warning: 2, info: 3 };

/** The worst first, then the longest waiting. */
export function byUrgency(left: Incident, right: Incident): number {
  return SEVERITY_RANK[left.severity] - SEVERITY_RANK[right.severity] || left.createdAt - right.createdAt;
}

export function countUnread(items: readonly AppNotification[], read: ReadonlySet<string>): number {
  return items.filter((item) => !read.has(item.id)).length;
}

/** The most ids kept: the store takes a bounded list. */
export const MAX_READ_NOTIFICATIONS = 200;

/** What to remember after reading `ids`: what is still listed and read, so the list never grows without end. */
export function settleRead(
  items: readonly AppNotification[],
  read: ReadonlySet<string>,
  ids: readonly string[],
): string[] {
  const listed = new Set(items.map((item) => item.id));
  const kept = [...read].filter((id) => listed.has(id));
  return [...new Set([...kept, ...ids])].slice(-MAX_READ_NOTIFICATIONS);
}

export function parseReadNotifications(raw: unknown): { ids: string[] } | null {
  const ids = (raw as { ids?: unknown } | null)?.ids;
  return Array.isArray(ids) && ids.every((id) => typeof id === 'string') ? { ids } : null;
}
