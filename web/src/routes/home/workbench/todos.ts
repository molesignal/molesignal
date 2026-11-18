import type { OwnedActionItem } from '@/api/postmortems';
import { type AppNotification, byUrgency, isWaitingOn } from '@/shell/notifications/model';
import type { Incident } from '@/types/alerting';

export type Todo =
  | { kind: 'incident'; key: string; incident: Incident }
  | { kind: 'approval'; key: string; title: string; description?: string | undefined; to?: string | undefined }
  | { kind: 'action_item'; key: string; item: OwnedActionItem; overdue: boolean }
  | { kind: 'stream_gap'; key: string; count: number }
  | { kind: 'activation'; key: string; remaining: number; nextLabel: string | null };

export interface TodoSources {
  userId: string;
  nowMicros: number;
  incidents: readonly Incident[];
  /** The bell's notifications: those asking for a decision are to-dos. */
  notifications: readonly AppNotification[];
  actionItems: readonly OwnedActionItem[];
  /** Streams that stopped receiving, for whoever looks after them; 0 when none or not theirs. */
  streamGaps: number;
  /** The setting-up steps left; `null` once they are known to be done or cannot be known. */
  activation: { remaining: number; nextLabel: string | null } | null;
}

/**
 * What waits on the reader, in the order it should be done: incidents waiting on them, the worst
 * and longest waiting first; decisions others wait for; action items past their date; streams that
 * stopped; the other action items, the soonest due first; and setting up, last.
 */
export function buildTodos(sources: TodoSources): Todo[] {
  const incidents: Todo[] = sources.incidents
    .filter((incident) => isWaitingOn(incident, sources.userId))
    .sort(byUrgency)
    .map((incident) => ({ kind: 'incident', key: `incident:${incident.id}`, incident }));

  const approvals: Todo[] = sources.notifications
    .filter((notification) => notification.category === 'approvals')
    .map((notification) => ({
      kind: 'approval',
      key: notification.id,
      title: notification.title,
      description: notification.description,
      to: notification.to,
    }));

  const items = sources.actionItems.map(
    (item): Todo => ({
      kind: 'action_item',
      key: `action:${item.id}`,
      item,
      overdue: item.dueAt !== null && item.dueAt < sources.nowMicros,
    }),
  );
  const overdue = items.filter((todo) => todo.kind === 'action_item' && todo.overdue);
  const due = items.filter((todo) => todo.kind === 'action_item' && !todo.overdue);

  const gaps: Todo[] =
    sources.streamGaps > 0 ? [{ kind: 'stream_gap', key: 'stream-gap', count: sources.streamGaps }] : [];

  const activation: Todo[] =
    sources.activation && sources.activation.remaining > 0
      ? [{ kind: 'activation', key: 'activation', ...sources.activation }]
      : [];

  return [...incidents, ...approvals, ...overdue, ...gaps, ...due, ...activation];
}
