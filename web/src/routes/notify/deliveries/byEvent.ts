import type { NotifyDelivery } from '@/api/notify';

export interface ChannelAttempts {
  connectorId: string | null | undefined;
  /** Oldest first. */
  attempts: NotifyDelivery[];
}

export interface EventDeliveries {
  eventId: string;
  /** When the first notification of this alert was made. */
  firstAt: number;
  channels: ChannelAttempts[];
  /** From the first notification to the first that arrived, in micros; absent while none has. */
  firstDeliveredAfter: number | null;
  failed: number;
}

function arrived(delivery: NotifyDelivery): boolean {
  return delivery.status === 'success' || delivery.status === 'acknowledged';
}

/**
 * One row to an alert instead of one to an attempt: each channel it went through,
 * with its tries in order, how long until someone could first have read it, and
 * how many tries failed. The list is the newest alert first.
 */
export function groupByEvent(rows: readonly NotifyDelivery[]): EventDeliveries[] {
  const byEvent = new Map<string, NotifyDelivery[]>();

  for (const row of rows) {
    const list = byEvent.get(row.eventId);
    if (list) list.push(row);
    else byEvent.set(row.eventId, [row]);
  }

  return [...byEvent]
    .map(([eventId, deliveries]) => {
      const ordered = [...deliveries].sort((a, b) => a.createdAt - b.createdAt || a.attempt - b.attempt);
      const firstAt = ordered[0]?.createdAt ?? 0;

      const channels = new Map<string, ChannelAttempts>();
      for (const delivery of ordered) {
        const key = delivery.connectorId ?? '';
        const channel = channels.get(key);
        if (channel) channel.attempts.push(delivery);
        else channels.set(key, { connectorId: delivery.connectorId, attempts: [delivery] });
      }

      const firstArrival = ordered
        .filter(arrived)
        .map((delivery) => delivery.sentAt ?? delivery.createdAt)
        .reduce<number | null>((earliest, at) => (earliest === null || at < earliest ? at : earliest), null);

      return {
        eventId,
        firstAt,
        channels: [...channels.values()],
        firstDeliveredAfter: firstArrival === null ? null : Math.max(0, firstArrival - firstAt),
        failed: ordered.filter((delivery) => delivery.status === 'failed').length,
      };
    })
    .sort((a, b) => b.firstAt - a.firstAt);
}
